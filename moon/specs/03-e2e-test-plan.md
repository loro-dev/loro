# 03. e2e 测试计划（最终验收方式）

本项目以 e2e 为最终验收：Rust 与 Moon 能互相 decode/encode，并由 Rust 侧验证语义一致。

## 3.1 总体思路

1. Rust 负责：
   - 构造覆盖面足够的 Loro 文档（不同容器、不同边界情况、多 peer）
   - 导出 blob（Snapshot/Updates/…）
   - 产出真值 JSON（`get_deep_value()`）
2. Moon 负责：
   - 对 blob 解析并重编码（transcode）
3. Rust 负责最终判定：
   - `import(Moon 输出 blob)` 成功
   - `get_deep_value()` 与真值 JSON 完全相等

## 3.2 测试向量（testdata）规范

建议目录：`moon/testdata/`

每个 case 一组文件：

- `<case>.blob`：Rust `export(...)` 的二进制输出
- `<case>.json`：Rust `get_deep_value().to_json_value()` 的 JSON（真值）
- `<case>.meta.json`（建议）：描述如何生成该 blob 的元信息，例如：
  - `mode`：snapshot / updates / shallow_snapshot / state_only / snapshot_at
  - `encode_mode`：3 或 4
  - `notes`：覆盖点说明（例如包含 emoji、触发 LZ4、触发 LargeValueBlock 等）
  - 对 updates：`from_vv` 或 `spans` 的构造参数（便于复现）

## 3.3 Rust 测试向量生成器（建议实现方式）

新增 Rust 小工具（可放在 `examples/` 或新增 crate），能力：

- `generate --out moon/testdata --seed ... --cases ...`
- 内置多组 case：
  - 基础容器：Map/List/Text/Tree/MovableList
  - Richtext Unicode：包含 emoji/非 BMP 字符
  - 大 value：触发 SSTable 多 block 与 LargeValueBlock
  - 压缩：确保产生 LZ4 压缩 block（并验证解压逻辑）
  - 多 peer：模拟协作写入
  - 导出模式：Snapshot / Updates(from vv) / UpdatesInRange / ShallowSnapshot / StateOnly / SnapshotAt

输出真值：

- 对 Snapshot：真值为导出目标版本的 `doc.get_deep_value()`
- 对 Updates：需要额外建立“回放场景”：
  - Rust 先生成基线 docA 与 docB（或 vv 起点），导出 updates blob
  - e2e 时 Rust 用基线 + import(updates) 得到目标状态，再与真值对比

## 3.4 Moon CLI 合约（供 Rust harness 调用）

建议固定为：

- `moon/bin/loro-codec transcode <in.blob> <out.blob>`

约束：

- `transcode` 必须：
  - 校验 checksum（失败返回非 0）
  - 正确处理 mode=3/4
  - 输出的 `<out.blob>` 必须是 Rust 可 import 的合法格式（checksum 也要正确）

可选 debug 命令（便于排查）：

- `decode <in.blob> --out <dir>`：输出解析后的结构化 JSON（例如 header、SSTable meta、ChangeBlock 段统计）

## 3.5 e2e 测试结构（推荐用 Rust integration tests 驱动）

伪流程：

1. Rust 遍历 `moon/testdata/*.blob`
2. 对每个 case：
   - 调用 Moon CLI：`transcode case.blob out.blob`
   - Rust 创建新 doc，`import(out.blob)`
   - 读取 `case.json` 真值并对比 `doc.get_deep_value()`

对 Updates 类 case：

- 测试应包含：
  - 基线状态（meta.json 指定）
  - import 顺序与前置版本 vector

## 3.6 覆盖矩阵（必须至少覆盖）

1. header：
   - magic 错误
   - checksum 错误
   - mode 不支持
2. SSTable：
   - 多 block
   - LZ4 压缩 block
   - LargeValueBlock
3. ChangeBlock：
   - 多 peer
   - dep flags / dep counters / lamport / timestamps 的 DeltaOfDelta
4. Value：
   - 所有 tag（0..16）
   - unknown tag（>=0x80）的保守重编码
5. 容器 state：
   - Map/List/Text/Tree/MovableList 全覆盖
   - Text 含 emoji（验证 Unicode scalar）

## 3.7 分阶段验收里程碑（建议）

1. Milestone 1：Rust→Moon→Rust（Snapshot only）e2e 通过
2. Milestone 2：FastUpdates e2e 通过
3. Milestone 3：ShallowSnapshot/StateOnly/SnapshotAt 覆盖通过
4. Milestone 4（可选）：编码策略对齐（压缩/块布局/byte-level 更接近 Rust）

## 3.8 当前落地（repo 现状）

目前 repo 内已落地一个 **可选** 的 Rust integration test（默认会在缺少 Moon/Node 时自动跳过）：

- Rust harness：`crates/loro/tests/moon_transcode.rs`
- Moon CLI：`moon/cmd/loro_codec_cli`（JS target，Node 侧用 `fs` 读写文件）

本地运行：

```sh
MOON_BIN=~/.moon/bin/moon NODE_BIN=node cargo test -p loro --test moon_transcode
```

当前覆盖点包含（至少）：

- Snapshot / AllUpdates
- SnapshotAt / StateOnly / ShallowSnapshot
- Updates(from vv)
- 多 peer（导出包含多个 peer 的 updates）

## 3.9 终极测试（两种形态，强一致性）

为避免“仅能 import 成功但语义细节漂移”，最终需要落地两类黄金测试：**同一份随机操作序列**下，Rust 与 Moon 的“结构化输出”要强一致。

> 约定：下文的 JsonUpdates 指 `loro::JsonSchema`（见 `docs/JsonSchema.md`）。

### 3.9.1 Updates：二进制 FastUpdates → JsonUpdates，一致性对照

目标：
- Rust 侧导出的 JsonUpdates，与 Moon 从**二进制 updates** 导出的 JsonUpdates **结构完全一致**。

建议流程：
1. Rust CLI（固定 seed，可复现）生成随机操作并 commit，得到最终 doc。
2. Rust 同时导出：
   - `updates.blob`：二进制 updates（mode=4，建议 `ExportMode::Updates{from: ...}` 或 `ExportMode::all_updates()`）
   - `updates.json`：JsonUpdates（Rust `export_json_updates(...)` 的 JSON 输出）
3. Moon 读取 `updates.blob`，导出 JsonUpdates（例如调用 Moon CLI `export-jsonschema`），得到 `updates.moon.json`。
4. 对比：`updates.moon.json` 与 `updates.json` 反序列化后的结构体/JSON 值完全相等（推荐**先 parse 再比较**，而不是比字符串）。

覆盖意义：
- 该测试直接约束 ChangeBlock/serde_columnar/Value 等解码语义：Moon 的“二进制→JsonUpdates”必须与 Rust 的“真值 JsonUpdates”一致。

### 3.9.2 Snapshot：二进制 FastSnapshot → Deep JSON(toJSON)，一致性对照

目标：
- Rust 的 `get_deep_value()` 真值 JSON，与 Moon 从**二进制 snapshot** 解析后产出的 deep JSON（toJSON）**结构完全一致**。

建议流程：
1. Rust CLI（固定 seed，可复现）生成随机操作并 commit，得到最终 doc。
2. Rust 同时导出：
   - `snapshot.blob`：二进制 snapshot（mode=3，`ExportMode::Snapshot`）
   - `snapshot.deep.json`：真值 JSON（`doc.get_deep_value().to_json_value()`）
3. Moon 读取 `snapshot.blob`，解析 snapshot，并提供 `toJSON`/`export-deep-json` 之类的接口输出 `snapshot.moon.deep.json`（最终状态的 JSON）。
4. 对比：`snapshot.moon.deep.json` 与 `snapshot.deep.json` 反序列化后的 JSON 值完全相等。

覆盖意义：
- 该测试直接约束 FastSnapshot 的 state snapshot（Map/List/Richtext/Tree/MovableList/Counter 等）解码语义与 JSON 形态，能暴露 Unicode scalar、排序/稳定性等问题。

当前落地（repo 现状）：
- Rust 生成器：`crates/loro/examples/moon_golden_gen.rs`
- Rust tests：`crates/loro/tests/moon_transcode.rs`（`moon_golden_updates_jsonschema_matches_rust` / `moon_golden_snapshot_deep_json_matches_rust`）
- Moon CLI：`moon/cmd/loro_codec_cli`（`export-jsonschema` / `export-deep-json`）
