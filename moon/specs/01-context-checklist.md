# 01. Context 收集清单（开工前必须完成）

本清单用于确保实现过程不因缺失背景导致反复返工。每一项都应当在实现前完成确认，并把结论记录到 `moon/SPEC_NOTES.md`（后续实现查阅用）。

## 1.1 规格文档（必须）

阅读并提取“可实现的确定性规则”：

- 主规格：`docs/encoding.md`
  - header / checksum / mode
  - FastSnapshot / FastUpdates
  - SSTable
  - OpLog KV schema（vv/fr/sv/sf + change blocks）
  - ChangeBlock 的整体结构（postcard + 列编码）
  - 自定义 Value Encoding（tag + payload）
  - serde_columnar 的外层格式与策略说明（BoolRle/Rle/DeltaRle/DeltaOfDelta）
- 补充规格：
  - `docs/encoding-xxhash32.md`：xxHash32 实现与 test vectors
  - `docs/encoding-lz4.md`：LZ4 Frame（以及 block 级解码）
  - `docs/encoding-container-states.md`：Map/List/Text/Tree/MovableList/Counter 的 state snapshot

输出物：

- `moon/SPEC_NOTES.md` 至少包含：
  - 所有端序规则（并注明出自哪段规格）
  - LEB128 与 postcard varint 的差异与各自使用场景
  - ContainerType 两套映射表
  - Richtext 的 Unicode 规则与 Moon 侧实现策略
  - 允许/不允许的宽容解析点（例如未知 value tag 的处理）

## 1.2 Rust 源码“真值”定位（必须）

每一块格式都要找到 Rust 参考实现并记录文件位置（方便对照/排查）：

- 顶层 header/body：`crates/loro-internal/src/encoding.rs`
- FastSnapshot / FastUpdates：`crates/loro-internal/src/encoding/fast_snapshot.rs`
- SSTable：
  - `crates/kv-store/src/sstable.rs`
  - `crates/kv-store/src/block.rs`
- ChangeBlock：
  - `crates/loro-internal/src/oplog/change_store/block_encode.rs`
  - `crates/loro-internal/src/oplog/change_store/block_meta_encode.rs`
- 自定义 Value：`crates/loro-internal/src/encoding/value.rs`
- ID / ContainerID：`crates/loro-common/src/lib.rs`
- ContainerWrapper：`crates/loro-internal/src/state/container_store/container_wrapper.rs`
- 各容器 state：
  - `crates/loro-internal/src/state/map_state.rs`
  - `crates/loro-internal/src/state/list_state.rs`
  - `crates/loro-internal/src/state/richtext_state.rs`
  - `crates/loro-internal/src/state/tree_state.rs`
  - `crates/loro-internal/src/state/movable_list_state.rs`
  - `crates/loro-internal/src/state/counter_state.rs`

输出物：

- 在 `moon/SPEC_NOTES.md` 里按模块建立“规格段落 ↔ Rust 源码位置”的映射索引。

## 1.3 Moonbit 语言/运行时能力确认（必须）

编码格式实现会依赖以下能力；需要提前确认 Moonbit 是否支持、或需要手写替代：

1. 整数：
   - 是否有 `Int64/UInt64`？
   - 位运算与移位的行为（逻辑/算术移位，溢出是否截断）？
2. 更大整数：
   - serde_columnar 的 `DeltaRle` 规范使用 i128 delta（至少需要能精确表示 i128）。
   - 若无 i128：是否有 BigInt？或者可用“有符号 128 位结构体（hi/lo）”实现？
3. 字节与切片：
   - `Bytes`/`Array[Byte]` 的拷贝成本与切片语义（零拷贝/拷贝）？
   - 如何实现安全的 reader（越界错误而非 panic）？
4. 浮点：
   - 是否可按字节读写 IEEE754 f64（LE/BE）？
5. Unicode：
   - 字符串是否支持按 Unicode scalar 遍历？
   - 如何从字符串中按“Unicode scalar count”截取子串（Richtext span.len 需要）？

输出物：

- `moon/specs/02-module-plan.md` 中 i128 与 Unicode 的实现选型必须基于此处结论。

## 1.4 对照数据与验收方式确认（必须）

为避免“实现了但不知道对不对”，必须在开工前确定：

- Rust 侧怎么生成测试向量（blob + 真值 JSON + 元信息）
- Moon 侧怎么运行单测与 e2e（至少提供 CLI 入口，供 Rust harness 调用）
- e2e 的判定方式：以 Rust `import()` 后 `get_deep_value()` 的 JSON 对比为准

输出物：

- `moon/specs/03-e2e-test-plan.md`（详细定义向量格式与 CLI 合约）
