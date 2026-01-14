# 02. 按模块逐步实现计划（含测试与退出条件）

本计划的核心原则：

1. **先底层 primitive，再组合结构**（否则排查成本极高）
2. **每个模块都有独立单测**（至少包含：样例向量 + 往返测试）
3. **尽早引入跨语言对照**（Rust 生成/验证，Moon 负责解析/编码）

下文每个模块包含：

- 目标：本模块做什么
- 依赖：必须先完成哪些模块
- 实现要点：容易错的点
- 测试：需要写哪些测试（单测/对照/属性/边界）
- 退出条件：达到什么程度才算“完成并可进入下一模块”

---

## 2.1 工程骨架与公共设施

### 模块：`errors` / `bytes_reader` / `bytes_writer`

目标：
- 统一错误类型（DecodeError/ChecksumMismatch/Unsupported/Overflow/InvalidInput 等）
- 提供安全的字节读写器：端序读取、切片、剩余长度、越界检查

测试：
- 越界读取必须返回错误（不允许崩溃）
- u16/u32/u64 的 LE/BE 读写往返

退出条件：
- 所有基础 IO API 在单测覆盖下稳定

---

## 2.2 整数编码：LEB128 与 postcard varint（必须严格区分）

### 模块：`leb128`

目标：
- ULEB128(u64) 与 SLEB128(i64) 的编码/解码

实现要点：
- SLEB128 是 two's complement sign extension（不是 zigzag）
- 必须设置最大读取字节数防止恶意输入导致死循环

测试：
- 使用 `docs/encoding.md` 的示例向量（含负数）
- 随机往返：encode->decode 等价（限制范围）

退出条件：
- 与规格示例完全一致；边界条件（0、最大值、最小值）通过

### 模块：`postcard/varint` + `postcard/zigzag`

目标：
- unsigned varint（用于 u16-u128/usize 等）
- zigzag（用于 i16-i128/isize 等）

测试：
- Rust 侧生成（postcard）随机 i64/u64 的二进制，Moon 解码一致
- Moon 编码后 Rust 解码一致

退出条件：
- i64/u64/usize 的对照测试稳定通过

---

## 2.3 校验与压缩：xxHash32 / LZ4 Frame

### 模块：`xxhash32`

目标：
- 按 `docs/encoding-xxhash32.md` 实现 xxHash32（seed=0x4F524F4C）

测试：
- 直接使用文档 test vectors（空输入、单字节、16 字节等）
- Rust 对照：随机 bytes 的 hash 值一致

退出条件：
- 文档向量 + Rust 对照测试全部通过

### 模块：`lz4_frame`（优先实现解压）

目标：
- 解析 LZ4 Frame（magic/descriptor/blocks/end mark）
- 解压 block（支持 overlap copy）

实现要点：
- Loro SSTable block 压缩使用 LZ4 Frame（不是 raw block）
- 先实现解压即可；编码阶段可先不压缩（输出 None），后续再补压缩

测试：
- Rust 生成的 LZ4 Frame 数据解压后与 Rust 解压一致
- 恶意输入：magic 错误、block size 越界、offset 溢出等必须报错

退出条件：
- 能解压 Rust 真实 SSTable block（含 LZ4）且对照一致

---

## 2.4 SSTable（KV Store）

### 模块：`sstable/*`

目标：
- 支持 SSTable `import_all`：解析 header、BlockMeta、blocks（Normal/Large）、校验 checksum、解压、还原 KV
- 支持 SSTable `export_all`：至少生成 Rust 可读的 SSTable（可先不压缩）

依赖：
- `bytes_reader`、`xxhash32`、`lz4_frame`

实现要点：
- BlockMeta checksum 覆盖范围：meta entries（不含 count）
- block checksum：对“压缩后/未压缩的 block body”做 xxHash32，再追加 checksum（checksum 本身不压缩）
- NormalBlock 的 key 前缀压缩：第一条 key 来自 BlockMeta.first_key；后续用 common_prefix_len + suffix 还原

测试：
- Moon 自构造 SSTable（无压缩/少量 KV）→ decode 得 KV 列表
- Rust 生成复杂 SSTable（多 block、LargeValueBlock、LZ4 压缩）→ Moon decode 得到 KV 列表与 Rust 对照一致
- Moon encode 的 SSTable → Rust `import_all` 成功，KV 迭代一致

退出条件：
- SSTable decode/encode 在跨语言对照下稳定通过

---

## 2.5 顶层 header/body（Document blob）

### 模块：`document`

目标：
- 解析 header：magic `"loro"`、checksum（xxHash32）、mode(u16 BE)
- 支持：
  - FastSnapshot body（三段 u32_le len + bytes；state 可为单字节 `"E"`）
  - FastUpdates body（重复 ULEB128 len + block bytes 到 EOF）

依赖：
- `xxhash32`、`leb128`

测试：
- checksum 正确/错误分支
- mode 不支持分支（1/2 必须报错）
- snapshot 空 state `"E"` 分支

退出条件：
- 可对 Rust 导出的 blob 完成 header/body 切分与校验

---

## 2.6 基础标识结构：ID / ContainerID / ContainerWrapper

### 模块：`id` / `container_id` / `container_wrapper`

目标：
- 实现：
  - ChangeBlock key：12 bytes（peer u64 BE + counter i32 BE）
  - ContainerID.to_bytes（root 与 normal）
  - postcard `Option<ContainerID>` 的历史 ContainerType 映射（只用于 wrapper.parent）
  - ContainerWrapper：首字节 type(to_bytes 映射) + depth(LEB128) + parent(postcard Option) + payload

测试：
- Rust 随机生成 ContainerID：
  - to_bytes 一致
  - postcard 序列化下 parent 的映射一致

退出条件：
- ContainerID/Wrapper 可以无歧义解析并可重编码

---

## 2.7 serde_columnar（列式编码）

### 模块：`serde_columnar/*`

目标：
- outer format：`varint(n_cols)` + N 次 `(varint(len), bytes)`
- strategy：
  - BoolRle
  - Rle（AnyRle）
  - DeltaRle（delta + AnyRle<i128>）
  - DeltaOfDelta（bitstream + prefix code）

依赖：
- `postcard/varint`、`postcard/zigzag`、`bytes_reader`

实现要点：
- Row count 不显式存储，必须从 column payload 解码时推导
- DeltaOfDelta 的 bit 顺序（big-endian bit order）极易错，必须重测
- i128 支持是硬门槛：需要明确 Moon 侧实现方案（BigInt 或自定义 128 位整数）

测试：
- 每个 strategy 的小向量单测（覆盖 run/literal/空序列/单元素）
- Rust serde_columnar 生成列数据 → Moon 解码一致
- Moon 编码 → Rust 解码一致（可分阶段完成：先 BoolRle/Rle/DeltaRle，再 DeltaOfDelta）

退出条件：
- 至少 ChangeBlock/State 用到的列结构都能稳定 decode；encode 分阶段补齐

---

## 2.8 自定义 Value Encoding（非 postcard 的那套）

### 模块：`value_custom`

目标：
- 实现 `docs/encoding.md` 的 Value Encoding（tag 0..16 + >=0x80 unknown）

实现要点：
- F64 big-endian
- I64/DeltaInt 使用 SLEB128（不是 zigzag）
- 对未知 tag：保留为 opaque（tag + raw payload）以便重编码

测试：
- Rust 生成覆盖所有 tag 的向量（含边界：NaN/Inf/-0.0、大整数、长字符串、二进制）
- Moon decode→encode→Rust decode 一致（语义）

退出条件：
- 作为 ChangeBlock values 段的基础依赖可用

---

## 2.9 ChangeBlock（FastUpdates 的核心）

### 模块：`change_block`

目标：
- 完整实现 ChangeBlock 的 decode（必要时 encode）

分解实现顺序（建议严格按层次）：
1. postcard 外层 EncodedBlock struct：切分出各 bytes 段
2. header 段（peer table、atom lens、deps、lamport）
3. change_meta 段（timestamps、commit msg lens + 拼接区）
4. arena：cids（ContainerArena）、keys、positions（PositionArena）
5. ops（serde_columnar）
6. delete_start_ids（serde_columnar）
7. values（自定义 Value）

测试：
- Rust 导出 FastUpdates：
  - Moon 能逐 block 解码
  - Moon encode 回 blob 后 Rust import 成功
  - Rust `get_deep_value()` 与真值 JSON 相同

退出条件：
- FastUpdates e2e 通过（至少覆盖多 peer + 多容器）

---

## 2.10 State（FastSnapshot 的 state_bytes）

### 模块：`state/*`

目标：
- 解码 `encoding-container-states.md` 中的各容器 state snapshot
- 作为“可重编码 IR”的一部分（至少可保留原始 bytes 以重编码）

实现顺序建议：
1. MapState
2. ListState
3. RichtextState（重点：Unicode scalar）
4. TreeState（PositionArena）
5. MovableListState（sentinel + visible/invisible 逻辑）
6. CounterState（如需）

测试：
- 每个容器都有 Rust 生成的专用向量（blob + 真值 JSON）
- Moon transcode 后 Rust import，deep json 等价

退出条件：
- FastSnapshot e2e 通过（覆盖全部容器类型）

---

## 2.11 CLI 与集成

### 模块：`moon/bin/loro-codec`

目标：
- 提供给 e2e harness 调用的稳定接口（建议至少包含）：
  - `decode <in.blob> --out <dir>`：输出结构化 JSON（debug 用）
  - `encode <in.json> --out <out.blob>`：从 IR JSON 生成 blob（进阶）
  - `transcode <in.blob> <out.blob>`：decode→encode（e2e 主入口）

测试：
- CLI 参数错误处理
- 端到端：对 `moon/testdata/*.blob` 逐个 transcode，Rust import 校验通过

退出条件：
- `transcode` 成为稳定可用的 e2e 入口
