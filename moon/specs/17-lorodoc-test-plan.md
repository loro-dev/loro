# 17. LoroDoc Runtime 测试与验收计划（Rust 作为 Oracle）

本文件定义 MoonBit LoroDoc runtime 的测试策略：单元测试、对照测试、并发场景最小集合，以及推荐的向量/CLI 合约。

目标：以 Rust Loro 的行为为真值（oracle），保证 Moon runtime 在 Map/List/MovableList/Text/Tree 上与 Rust/TS 一致收敛。

---

## 17.1 测试层级

### L1：纯单元测试（Moon）

覆盖纯函数/可孤立模块：

- `ID/IdLp/VersionVector` 的比较与 includes 语义（11-core-model）
- Root name 校验（11-core-model）
- ContainerID string/bytes roundtrip（若 runtime 复用 codec，可只测 string 侧）
- DeleteSpan 的 `start/end/is_reversed` 等（12-seq-crdt）
- FractionalIndex 生成与 `generate_n_evenly`（15-tree）
- RichText 的 unicode/utf8/utf16 换算（14-richtext）

### L2：确定性场景测试（Moon）

用固定脚本驱动 Moon runtime，断言 `toJSON` 输出：

- Map：单 key set/delete，覆盖 LWW tie-break（同 lamport 不同 peer）
- List：insert/delete 边界与并发插入顺序（可通过导入 Rust 生成 updates）
- MovableList：move/set 与 delete+insert 的区别（避免冗余）
- Text：insert/delete，mark/unmark + expand（before/after/both/none）
- Tree：create/move/delete、rearrange、cycle 防护

### L3：差分测试（Rust ↔ Moon）

Rust 作为 oracle：

1. Rust 生成 updates + 真值 deep JSON
2. Moon import updates 后输出 deep JSON
3. 结构化比较（忽略字段顺序）

以及反向：

1. Moon 生成 updates
2. Rust import
3. deep JSON 对比

---

## 17.2 推荐向量格式（Spec-driven）

建议在 `moon/specs/vectors/lorodoc/` 下存放每个用例一个目录：

```
case-0001-basic-map/
  updates.bin        # mode=4 updates
  expected.json      # Rust get_deep_value().to_json_value()
  meta.json          # 可选：peer ids / 生成脚本信息
```

并发用例建议额外记录两个方向的 updates：

```
case-0100-concurrent-list-insert/
  a-to-b.bin
  b-to-a.bin
  expected.json
```

> 说明：此处的向量是“runtime 层”的，不是 codec 的 transcode 向量；它要求 Moon 能 import 并计算 deep value。

---

## 17.3 并发场景最小集合（必须覆盖）

### 17.3.1 Map（LWW）

- 两端并发 `map.set("k", 1)` 与 `map.set("k", 2)`，交换 updates，结果按 `(lamport,peer)` 决胜
- 并发 `set` 与 `delete`，确保 delete 也是一次写入（LWW）

Rust 参考：`MapState` 的 compare 逻辑（`map_state.rs`）。

### 17.3.2 List（seq-crdt）

- 并发在同一 index 插入不同值（如 A 插 "A"，B 插 "B"）
  - 断言最终顺序与 Rust 一致（通常由 peer/origin tie-break 决定）
- 并发 insert 与 delete 的交织（覆盖 DeleteSpan reverse/merge 情况）

Rust 参考：`CrdtRope::insert/delete` + `ListDiffCalculator`。

### 17.3.3 MovableList（identity）

必须覆盖与 List 的差异点：

- `move` 与并发 `set`：同一 elem_id 上 move/set 交织，最终位置与值按 IdLp LWW
- `set` 并发：同一 elem_id 两端 set，不产生冗余元素
- `move` 并发：同一 elem_id 两端 move，最终只有一个位置生效（另一个 move 留下的记录不可见）
- `move` vs `delete` 并发：delete 移除 elem 后，后续/并发 move 应无效或不可见（与 Rust 对齐）

Rust 参考：`MovableListDiffCalculator`（pos/value LWW）与 `tracker.move_item`。

### 17.3.4 Text（RichText）

- 并发 insert：同一位置插入不同字符串
- mark expand：
  - after：插在 range.end 边界应继承
  - before：插在 range.start 边界应继承
  - none：边界插入不继承
  - both：两侧都继承
- 并发 mark/unmark（同 key）

Rust 参考：`TextStyleInfoFlag::prefer_insert_before` + richtext_state。

### 17.3.5 Tree（movable tree）

- 并发 move：同一节点不同 parent/index，按 last-move-wins
- 并发导致 cycle（A: x→y，B: y→x），确保无 cycle（某个 move 被忽略）
- rearrange：大量插入/移动导致 fi 冲突，触发 generate_n_evenly 后顺序稳定

Rust 参考：`TreeState::mov(with_check)` + `NodeChildren::generate_fi_at`。

---

## 17.4 推荐 CLI 合约（后续实现）

为了让 Rust harness 直接驱动 Moon runtime，建议新增 CLI：`moon/cmd/loro_doc_cli/`：

- `import --in updates.bin --out out.json`
  - 读取 mode=4 updates，Moon import，输出 deep JSON
- `apply-script --in ops.json --out updates.bin --out-json out.json`
  - 读取操作脚本（自定义 JSON），在 Moon runtime 执行并导出 updates + deep JSON

Rust harness 侧：

- 生成 ops 脚本或直接在 Rust 执行并导出 updates.bin + expected.json
- 调用 Moon CLI，比较 out.json 与 expected.json

---

## 17.5 通过标准（合并 gate）

必须满足：

- 所有 L2 用例通过
- L3 差分测试在固定种子下跑过（至少每类容器 100–1000 步的随机操作；并发至少 20 组）
- 任何差异必须能定位到：导入顺序、LWW tie-break、seq-crdt 插入排序、expand 行为、或 tree cycle/rearrange 之一

