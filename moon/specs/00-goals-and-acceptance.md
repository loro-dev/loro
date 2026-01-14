# 00. 目标、范围与验收标准

## 0.1 背景

Loro 的导出/导入使用二进制编码格式（见 `docs/encoding.md` 及其补充文档）。本项目目标是在 Moonbit 中实现同等格式的编解码，从而实现与 Rust 版 Loro 的互操作。

## 0.2 最终验收（必须满足）

### A. Rust → Moon：任意导出 blob 可正确 decode

对任意 Rust 侧 `LoroDoc.export(...)` 产物（目前主要是 FastSnapshot 与 FastUpdates）：

- Moon 解码必须：
  - 校验 magic、mode、checksum
  - 正确解析 body（snapshot/updates）
  - 正确解析 SSTable、ChangeBlock、容器 state、值编码、压缩（LZ4 Frame）
  - 不应依赖“仅某些用例”的固定结构

### B. Moon → Rust：Moon 编码产物可被 Rust 正确 import

Moon 需要能把解码得到的结构重新编码为合法 blob，并满足：

- Rust `import()` 成功
- Rust 导入后文档状态与预期一致（由 Rust 真值/对照判定）

> 备注：初期可以不要求 **字节级完全一致**（byte-for-byte），但必须保证语义一致；后续可逐步追求 byte-level 相等以减少差异风险。

### C. 双向互通 e2e

项目“完成”的验收方式为 e2e 测试：

1. Rust 生成测试文档与导出 blob（Snapshot/Updates/…）。
2. Moon 对 blob 执行 `decode`（必要时 `transcode`：decode→encode）。
3. Rust `import()` Moon 产物并对比 `get_deep_value()`（或同等）与真值 JSON。

## 0.3 编码模式支持范围

- 必须支持：
  - `EncodeMode = 3`：FastSnapshot
  - `EncodeMode = 4`：FastUpdates
- 明确不支持并报错：
  - `EncodeMode = 1`：OutdatedRle
  - `EncodeMode = 2`：OutdatedSnapshot

## 0.4 兼容性定义（实现策略建议）

为了保证“可重编码”与跨语言互通，建议在 Moon 侧维护一个 **Lossless IR**：

- IR 的目标不是实现完整 CRDT 行为，而是能承载：
  - 文档 header（mode、checksum 可重算）
  - Snapshot：oplog_bytes（SSTable）+ state_bytes（SSTable 或 empty “E”）+ shallow_root_state_bytes（可为空）
  - Updates：ChangeBlock 序列
  - SSTable：块、meta、每个 KV 条目
  - ChangeBlock：各段（header/meta/cids/keys/positions/ops/delete_start_ids/values）能 lossless 还原
  - 未知/未来字段：以 opaque bytes 形式保留（保证 forward compatibility 的“保守重编码”）

阶段性里程碑建议：

1. **v1：转码器（transcoder）优先**：Moon 先做到能把 Rust blob decode 成 IR，再 encode 回可被 Rust import 的 blob（允许编码细节与 Rust 不同，如压缩策略/块切分不同）。
2. **v2：对齐增强**：逐步对齐 Rust 的编码策略（block_size、压缩开关、列编码细节），降低差异与边界风险。

## 0.5 关键正确性注意点（必须写入测试用例）

1. **checksum 覆盖范围**：文档头的 xxHash32 覆盖 bytes[20..]（包含 2 字节 mode + body），不是仅 body。  
2. **端序陷阱**：
   - header mode 是 **u16 big-endian**
   - `ID.to_bytes()`（peer+counter）是 **big-endian**
   - 自定义 Value 编码里的 F64 是 **big-endian**
   - postcard 的 f64 是 **little-endian**
3. **整数编码混用**：
   - LEB128（ULEB128/SLEB128）用于 Value 编码等
   - postcard 使用 unsigned varint + zigzag（不是 SLEB128）
4. **两套 ContainerType 映射**：ContainerWrapper 的首字节与 postcard 序列化的历史映射不同（必须在解码 parent `Option<ContainerID>` 时用历史映射）。
5. **Richtext span.len 的语义**：是 Unicode scalar count（不能用 UTF-16 code unit 直接切）。

## 0.6 非目标（可选但不强制）

- 在 Moon 内实现完整 Loro CRDT 与操作应用（本项目只要求编码格式互通）。
- 完全复刻 Rust 的压缩策略与块布局（byte-for-byte 相同）。可作为后续优化目标。
