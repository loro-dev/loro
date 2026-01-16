# 05. FastUpdates（mode=4）/ ChangeBlock 编码：Context 与实现规格摘记

本文件用于支撑 MoonBit 侧把 **FastUpdates** 做到真正的 `decode -> Change/Op -> encode`（而不是仅校验后原样输出 bytes），从而满足“Rust ↔ Moon 任意导出格式都能互相 decode/encode”的最终目标。

> 重要经验：`docs/encoding.md` 有部分细节会滞后或不完整；这里以 Rust 源码为真值，并把实现时容易踩坑的点显式写下来，避免反复试错。

## 5.1 真值来源（必须读）

- ChangeBlock 打包与 op 编码：
  - `crates/loro-internal/src/oplog/change_store/block_encode.rs`
  - `crates/loro-internal/src/encoding/outdated_encode_reordered.rs`（`get_op_prop` / `encode_op` 真正决定 `prop` 与 `value_type`）
- ChangeBlock header/meta 编码：
  - `crates/loro-internal/src/oplog/change_store/block_meta_encode.rs`（`encode_changes`）
- Arena：
  - `crates/loro-internal/src/encoding/arena.rs`（`ContainerArena` / `PositionArena`）
- Value 编码（values 段）：
  - `crates/loro-internal/src/encoding/value.rs`（`ValueWriter` / `ValueKind`）
- 依赖库语义（排查策略/空序列编码时用）：
  - `serde_columnar` 0.3.14（策略与 wrapper 行为）
  - `postcard`（struct/vec/bytes 的编码语义）

## 5.2 EncodedBlock（最外层：postcard struct）

Rust 侧 `EncodedBlock<'a>`（见 `block_encode.rs`）被 postcard 序列化为：

1. `counter_start: u32`（postcard varint(u64) 形式承载，要求 ≤ u32）
2. `counter_len: u32`
3. `lamport_start: u32`
4. `lamport_len: u32`
5. `n_changes: u32`
6. `header: bytes`（postcard bytes：`varint(len) + raw`）
7. `change_meta: bytes`
8. `cids: bytes`
9. `keys: bytes`
10. `positions: bytes`
11. `ops: bytes`
12. `delete_start_ids: bytes`
13. `values: bytes`

字段顺序非常关键：Moon 侧的 `decode_encoded_block/encode_encoded_block` 必须与之对应。

### 关键派生字段（由 Change 列表推导）

在 Rust `encode_block` 中：

- `counter_start = first_change.id.counter`
- `counter_len = last_change.ctr_end() - first_change.id.counter`
- `lamport_start = first_change.lamport()`
- `lamport_len = last_change.lamport_end() - first_change.lamport()`
- `n_changes = block.len()`

Moon 侧实现 `encode_change_block(changes)` 时应使用同样定义。

## 5.3 header 段（`encode_changes` 输出的第一个 bytes）

来源：`block_meta_encode.rs::encode_changes`

布局（按顺序拼接）：

1. **Peer Table**
   - `peer_count: ULEB128(u64)`
   - `peer_ids: peer_count × u64_le`
   - 约束：`peers[0]` 必须是该 block 的 peer（Rust 在进入 encode_block 时先 `peer_register.register(&peer)` 保证）。

2. **AtomLen（只写 N-1 个）**
   - 对每个 change（除最后一个）：写 `atom_len` 的 `ULEB128(u64)`
   - 最后一个 change 的 atom_len 不写，解码侧通过 `counter_len - sum(prev)` 推导。

3. **Deps（按 change 展开）**
   - `dep_on_self: BoolRle`（长度 = N）
   - `dep_len: AnyRle<usize>`（长度 = N；为“去掉 self dep 后”的 deps 数）
   - `dep_peer_idx: AnyRle<usize>`（长度 = sum(dep_len)；peer_idx 指向 Peer Table）
   - `dep_counter: DeltaOfDelta<u32>`（长度 = sum(dep_len)；counter）

4. **Lamport（只写 N-1 个）**
   - `lamport: DeltaOfDelta<u32>`（长度 = N-1）
   - 最后一个 change 的 lamport 不直接编码，解码时用：
     - `last_lamport = lamport_start + lamport_len - last_atom_len`

> 坑点：header 里 lamport 的 DeltaOfDelta 只包含 N-1 个元素，这是最常见 off-by-one bug 源头之一。

## 5.4 change_meta 段（`encode_changes` 输出的第二个 bytes）

来源：`block_meta_encode.rs::encode_changes`

布局（按顺序拼接）：

1. `timestamps: DeltaOfDelta<i64>`（长度 = N）
2. `commit_msg_lens: AnyRle<u32>`（长度 = N；None → 0）
3. `commit_msgs: bytes`（把所有非空 commit_msg 直接拼接的 UTF-8 字节串）

解码时需要按 `commit_msg_lens` 切分末尾 bytes；编码同理。

## 5.5 keys 段（key_register 的输出）

来源：`block_encode.rs::encode_keys`

布局：重复直到 EOF：

- `len: ULEB128(u64)`
- `utf8_bytes: len`

注意：这里用的是 **ULEB128**（不是 postcard varint）。

## 5.6 cids 段（ContainerArena）

来源：`encoding/arena.rs::ContainerArena::encode`

**关键坑点：它不是 columnar vec。**

Rust 实际调用：`serde_columnar::to_vec(&self.containers)`，其中 `self.containers: Vec<EncodedContainer>`。
由于这是对 **Vec 直接做 serde/postcard 序列化**，结果是 row-wise 的 postcard Vec 结构：

- `vec_len: varint(u64)`
- 对每个元素（EncodedContainer，4 个字段）：
  - `field_count: varint(u64)`（固定为 `4`，postcard 对 “struct as seq” 的编码）
  - `is_root: u8`（0/1）
  - `kind: u8`（ContainerID.to_bytes 的映射：Map=0,List=1,Text=2,Tree=3,MovableList=4,Counter=5）
  - `peer_idx: varint(u64)`（root 时固定为 0；normal 时为 peers 表索引）
  - `key_idx_or_counter: zigzag-varint(i64)`（i32 范围）

语义映射：

- root：`(is_root=true, key_idx_or_counter = keys[name_idx])`
- normal：`(is_root=false, peer_idx -> peers[peer_idx], key_idx_or_counter = counter)`

实现建议：

- 在 `encode_change_block` 中，先收集所有涉及到的 ContainerID，建立 `keys` 和 `peers` 注册表，再按顺序生成 container_arena bytes。

## 5.7 positions 段（PositionArena v2）

来源：`encoding/arena.rs::PositionArena::encode_v2`

- 若 positions 为空：直接返回空 bytes（长度 0）
- 否则：`serde_columnar::to_vec(&PositionArena { positions: Vec<PositionDelta> })`
  - 包含 **struct wrapper**（field_count=1）+ columnar vec 两列：
    - `common_prefix_length: AnyRle<usize>`
    - `rest: bytes column`（注意 bytes column 自身内部有 count 与每段 length）

Moon 侧实现时要区分：

- ChangeBlock 的 positions：允许空 bytes
- TreeState 的 fractional_indexes：Rust 用 `PositionArena::encode()`，即使为空也会产生非空 payload（所以 Moon 侧需要单独的 `encode_position_arena()` 语义）

## 5.8 ops 段（EncodedOps）

来源：`block_encode.rs::EncodedOps`

Rust：`serde_columnar::to_vec(&EncodedOps { ops })`

编码为：

- struct wrapper：`field_count=1`
- columnar vec（4 列）：
  1. `container_index: DeltaRle<u32>`（container_arena 的索引）
  2. `prop: DeltaRle<i32>`
  3. `value_type: Rle<u8>`（`ValueKind::to_u8()`）
  4. `len: Rle<u32>`（op atom_len）

### prop 的计算（必须与 Rust 完全一致）

真值来源：`encoding/outdated_encode_reordered.rs::get_op_prop`

- List/MovableList/Text：
  - Insert/InsertText：`prop = pos`
  - Delete：`prop = pos`
  - MovableList Move：`prop = to`
  - MovableList Set：`prop = 0`
  - Text StyleStart：`prop = start`
  - Text StyleEnd：`prop = 0`
- Map：`prop = key_register.register(map.key)`（key_idx）
- Tree：`prop = 0`
- Future：
  - Counter：`prop = 0`
  - Unknown：`prop = op.prop`

## 5.9 delete_start_ids 段（EncodedDeleteStartIds）

来源：`block_encode.rs`

- 若该 block 没有任何 DeleteSeq：此段 **为 0 字节空串**（不是 “空 columnar vec”）
- 否则：`serde_columnar::to_vec(&EncodedDeleteStartIds { delete_start_ids })`
  - struct wrapper：field_count=1
  - columnar vec（3 列）：
    - `peer_idx: DeltaRle<usize>`
    - `counter: DeltaRle<i32>`
    - `len: DeltaRle<isize>`（Moon 侧建议用 i64 承载）

生成规则（真值来源：`encoding/outdated_encode_reordered.rs::encode_op`）：

- 每遇到一次 DeleteSeq（List/MovableList/Text）：
  - push `EncodedDeleteStartId { peer_idx = peer_register.register(id_start.peer), counter, len }`
  - values 段写 `Value::DeleteSeq`（无 payload）

## 5.10 values 段（ValueWriter 的输出）

来源：`encoding/value.rs` + `outdated_encode_reordered.rs::encode_op`

规则：

- values 段是按 ops 的顺序连续拼接的 Value 编码（每个 value 起始 1 byte tag）。
- `value_type` 列来自 `ValueKind`，必须与具体 value 的 tag 一致。

需要覆盖的 value 类型（对照 Moon 侧 `OpContent`）：

- Map：`LoroValue` / `DeleteOnce`
- List/MovableList：`LoroValue::List` / `DeleteSeq` / `ListMove` / `ListSet`
- Text：`Str` / `DeleteSeq` / `MarkStart` / `Null(=MarkEnd)`
- Tree：`RawTreeMove`（用于 Create/Move/Delete 的三态：Delete 用 deleted_root + position_idx=0）
- Future：counter 的 I64/F64 优化；unknown 的 opaque bytes

## 5.11 实现分阶段建议（避免一次性做完难排查）

1. **先做 “decode->encode 等价（语义）” 的 FastUpdates transcode**：
   - `parse_fast_updates_body` 拿到每个 block bytes
   - `decode_change_block(bytes)` -> `changes: Array[Change]`
   - `encode_change_block(changes)` -> `new_bytes`
   - 输出 new_bytes 组成新 document
2. 对每个段落单独做 “可视化对照”：
   - Rust 侧加 probe：打印各段 bytes hex（已证明 cids/ops 有坑）
   - Moon 侧增加 debug 命令输出（可选）以便 diff
3. 最后再追求 “byte-level 更接近 Rust”（例如压缩、block size、字段排序一致）。

