# 04. Moon 侧 Change / Op 数据结构设计

本文件定义 Moonbit 侧用于“可重编码（decode→encode）”与“可测试（对照/Golden）”的核心数据结构（`Change` / `Op` 等）。

目标：

1. **能承载 ChangeBlock 的核心语义**：Change 元数据 + Op 序列（含值、引用与删除跨度）。
2. **便于测试**：可序列化为稳定 JSON，用于与 Rust 侧导出的 JSON 对照（或作为 golden）。
3. **可用于编码**：从这些结构能构建回 ChangeBlock（不要求 byte-for-byte 相同，但必须 Rust 可 import 且语义一致）。

> 说明：下文类型用“Moonbit 风格伪代码”描述，最终落地时可按 Moonbit 实际语法调整，但字段语义/约束不应改变。

---

## 4.1 基础类型

### 4.1.1 数值类型约定

- `PeerID`：u64（Rust 为 `u64`）
- `Counter`：i32（Rust 为 `i32`）
- `Lamport`：u32（Rust 为 `u32`，但在若干 JSON/显示层可按 i64 表示）
- `Timestamp`：i64

> 注意：编码层同时存在两套变长整数体系：
> - 自定义 Value 编码用 LEB128（含 SLEB128）
> - postcard/serde_columnar 用 varint + zigzag
>
> 这些结构只关心语义值本身，不暴露编码细节。

### 4.1.2 ID / IdLp / TreeID

```
struct ID { peer: PeerID, counter: Counter }
struct IdLp { peer: PeerID, lamport: Lamport } // movable list 用
type TreeID = ID // Tree 节点 ID 与 ID 结构一致（peer+counter）
```

为了对照 Rust 的字符串格式，提供以下约定（仅用于 JSON/调试）：

- `ID` 字符串：`"{counter}@{peer}"`
- `IdLp` 字符串：`"L{lamport}@{peer}"`

### 4.1.3 ContainerID

```
enum ContainerType {
  Map, List, Text, Tree, MovableList,
  Counter,        // 可选 feature
  Unknown(u8),    // 未来扩展
}

enum ContainerID {
  Root { name: String, ty: ContainerType },
  Normal { peer: PeerID, counter: Counter, ty: ContainerType },
}
```

与 Rust 的 `Display/TryFrom<&str>` 对齐（用于 JSON/测试）：

- Root：`"cid:root-{name}:{ContainerType}"`
- Normal：`"cid:{ID}:{ContainerType}"`

其中 `ContainerType` 显示为：`Map/List/Text/Tree/MovableList/(Counter)/Unknown(k)`。

### 4.1.4 FractionalIndex（Tree position）

Tree 的 position 在二进制里是 `FractionalIndex` 的 bytes，JSON 侧使用其 `Display`：

- `fractional_index`：**大写十六进制**字符串（Rust `FractionalIndex::to_string()` 实际是 bytes 的 `%02X` 拼接）。

推荐存两份（便于编码与测试）：

```
struct FractionalIndex {
  bytes: Bytes,          // 编码用
  hex: String,           // 测试/日志用，可由 bytes 推导
}
```

---

## 4.2 LoroValue（用户态值，用于 Insert/Set/Mark 等）

LoroValue 在二进制里走 postcard（见 `docs/encoding-container-states.md` 的 “LoroValue Encoding (in postcard)”），在 JSON（human-readable）里走 Rust 自定义序列化规则（见 `crates/loro-common/src/value.rs`）：

- `Null` → JSON `null`
- `Bool` → JSON `true/false`
- `Double/I64` → JSON number
- `String` → JSON string
- `Binary` → JSON number array（0..255）
- `List` → JSON array
- `Map` → JSON object
- `Container(ContainerID)` → JSON string：`"🦜:" + ContainerIDString`

建议直接复用这个“测试友好 JSON 形态”（特别是容器引用前缀 `🦜:`），从而可与 Rust 输出直接对照。

---

## 4.3 Change（核心结构之一）

### 4.3.1 结构定义

```
struct Change {
  id: ID,                      // change 起始 ID（peer+counter）
  timestamp: i64,              // change timestamp（DeltaOfDelta）
  deps: Array[ID],             // frontiers（对照 Rust json_schema: deps）
  lamport: Lamport,            // change 的 lamport 起点
  msg: Option[String],         // commit message（None/Some）
  ops: Array[Op],              // op 列表（按 counter 递增）
}
```

### 4.3.2 约束（用于测试断言）

- `ops` 必须按 `op.counter` 递增排序。
- `op.counter` 必须满足：`id.counter <= op.counter < id.counter + change_op_len`。
- `change_op_len` 定义为 `sum(op_len(op.content))`，且应等于该 Change 在 ChangeBlock header 中的 atom_len。

> 注：FastUpdates 的 ChangeBlock header 对 “self dep” 做了压缩（dep_on_self），解码后 `deps` 应包含完整 dep 列表（含 self dep）。

---

## 4.4 Op（核心结构之二）

### 4.4.1 顶层结构

```
struct Op {
  container: ContainerID,   // 目标容器
  counter: Counter,         // op 的起始 counter（绝对值，不是相对 offset）
  content: OpContent,       // 语义操作
}

enum OpContent {
  List(ListOp),
  MovableList(MovableListOp),
  Map(MapOp),
  Text(TextOp),
  Tree(TreeOp),
  Future(FutureOp),         // Unknown/Counter（可选）
}
```

为了最大化测试复用，建议让 `OpContent` 的形态尽量与 Rust 的 `encoding/json_schema.rs::json::JsonOpContent` 对齐。

### 4.4.2 各容器 OpContent 详细定义与 op_len 规则

#### List

```
enum ListOp {
  Insert { pos: u32, value: Array[LoroValue] },
  Delete { pos: i32, len: i32, start_id: ID },
}
```

- `op_len(Insert) = value.length`
- `op_len(Delete) = abs(len)`（注意 len 可为负，代表不同方向，语义以编码规则为准）

#### MovableList

```
enum MovableListOp {
  Insert { pos: u32, value: Array[LoroValue] },
  Delete { pos: i32, len: i32, start_id: ID },
  Move { from: u32, to: u32, elem_id: IdLp },
  Set { elem_id: IdLp, value: LoroValue },
}
```

- Insert/Delete 的 `op_len` 同 List
- Move/Set 的 `op_len = 1`

#### Map

```
enum MapOp {
  Insert { key: String, value: LoroValue },
  Delete { key: String },
}
```

- `op_len = 1`

#### Text（Richtext ops）

```
enum TextOp {
  Insert { pos: u32, text: String },
  Delete { pos: i32, len: i32, start_id: ID },
  Mark { start: u32, end: u32, style_key: String, style_value: LoroValue, info: u8 },
  MarkEnd,
}
```

- `op_len(Insert) = unicode_scalar_count(text)`（必须与 Rust `text.chars().count()` 一致）
- `op_len(Delete) = abs(len)`
- `op_len(Mark) = 1`，`op_len(MarkEnd) = 1`

> 注意：编码层的 MarkStart 里带有 `len=end-start`，但它不等价于 atom_len；atom_len 固定为 1。

#### Tree

```
enum TreeOp {
  Create { target: TreeID, parent: Option[TreeID], fractional_index: FractionalIndex },
  Move   { target: TreeID, parent: Option[TreeID], fractional_index: FractionalIndex },
  Delete { target: TreeID },
}
```

- `op_len = 1`

#### Future（未知/扩展容器）

目标：提供可重编码的“保守”表示，保证未来版本不会把数据丢掉。

```
enum FutureOp {
  // 可选：counter feature
  Counter { value: EncodedValue }, // 值可能是 I64 或 F64
  Unknown { prop: i32, value: EncodedValue }, // value 用自定义 Value 编码体系
}
```

`EncodedValue` 建议对齐 Rust `encoding/value.rs::OwnedValue` 的 JSON 表示（`{ "value_type": "...", "value": ... }`），至少包含：

- `i64` / `f64` / `str` / `binary` / `loro_value` / `delete_once` / `delete_seq` / `delta_int`
- `mark_start` / `list_move` / `list_set` / `raw_tree_move`
- `future.unknown(kind,data)`：保留未知 kind 与原始 bytes（以便重编码）

---

## 4.5 Change / Op ↔ ChangeBlock（二进制）映射要点（用于实现与测试）

本节不是完整实现指南，而是把“字段如何从编码里来”与“编码时如何从字段生成”讲清楚，避免实现时失配。

### 4.5.1 解码（binary → Change/Op）关键路径

以 FastUpdates 的单个 ChangeBlock 为例：

1. postcard 解出 `EncodedBlock` 外层字段：
   - `counter_start/counter_len/lamport_start/lamport_len/n_changes`
   - 以及各 bytes 段：`header/change_meta/cids/keys/positions/ops/delete_start_ids/values`
2. 解析 `header`（见 `crates/loro-internal/src/oplog/change_store/block_meta_encode.rs::decode_changes_header`）：
   - 得到 `peers[]`、每个 change 的 `atom_len`、`deps`、每个 change 的 `lamport`
3. 解析 `change_meta`：
   - `timestamps[]`（DeltaOfDelta）
   - `commit_msg_len[]`（AnyRle<u32>）+ 拼接区 → `msg[]`
4. 解析 arenas：
   - `cids`：ContainerArena（postcard Vec<EncodedContainer>）→ `ContainerID[]`
   - `keys`：LEB128(len)+utf8 → `String[]`
   - `positions`：PositionArena（serde_columnar）→ `Bytes[]`
5. 解析 `ops`（serde_columnar EncodedOp 列）得到 `[(container_idx, prop, value_type, len)]`
6. 解析 `delete_start_ids`（serde_columnar）得到删除跨度表（供 DeleteSeq 消费）
7. 解析 `values`：按每个 op 的 `value_type` 顺序消费 values byte stream，得到 `Value`（自定义 Value 编码体系）
8. **用容器类型 + prop + value** 还原语义 Op（对照 Rust `crates/loro-internal/src/encoding/outdated_encode_reordered.rs::decode_op`）：
   - Map：`prop` 是 `key_idx` → `keys[key_idx]`
   - List/Text/MovableList：`prop` 多为位置；Delete 需要从 delete_start_ids 取 `start_id + signed_len`
   - Text Mark：由 `MarkStart` + `prop(start)` 还原 `start/end/style_key/style_value/info`
   - Tree：使用 `RawTreeMove` + `positions[position_idx]`；并需计算 `op_id` 来区分 Create/Move（见 Rust `is_create = subject.id() == op_id`）
9. 将 ops 按 change atom_len 切分到每个 `Change.ops`：
   - 对每个 change：累积 `op_len(op.content)` 直到等于该 change 的 atom_len
   - 同时填充 Change：`id/timestamp/deps/lamport/msg`

### 4.5.2 编码（Change/Op → binary）关键路径

编码时不要求与 Rust byte-for-byte 一致，但必须 Rust 可 import。建议“先做可用版，再做对齐版”：

- v1（可用版）：
  - 直接从 `Change/Op` 重建 registers（peer/key/cid/position），生成 ContainerArena/keys/positions，并生成 ops 列 + delete_start_ids + values bytes。
  - SSTable 的编码可统一用 `compression_type=None`（避免压缩差异）；ChangeBlock 内 values 不压缩。

从 `Change/Op` 构造 ChangeBlock 的关键点（对照 Rust `encode_op/get_op_prop/encode_block`）：

1. `container_idx`：来自 `cid_register.register(container_id)`
2. `prop`：按 op 类型计算（等价 Rust `get_op_prop`）：
   - List/MovableList Insert/Delete/InsertText：`prop = pos`
   - MovableList Move：`prop = to`
   - MovableList Set：`prop = 0`
   - Text Insert/Delete/Mark：`prop = pos/start`
   - Text MarkEnd：`prop = 0`
   - Map：`prop = key_idx`（key_idx 来自 key_register）
   - Tree：`prop = 0`
3. `value_type + values/delete_start_ids`：按 op 内容映射（等价 Rust `encode_op`）：
   - List/MovableList Insert → 写入 `LoroValue(list)` 到 values
   - Text Insert → 写入 `Str(text)` 到 values
   - Map Insert/Delete → 写入 `LoroValue(v)` 或 `DeleteOnce`
   - Delete → 写入 `DeleteSeq`（values）+ 追加一条 delete_start_id
   - Text Mark → 写入 `MarkStart`（含 len/end-start、key、value、info）
   - Text MarkEnd → 写入 `Null`
   - MovableList Move → 写入 `ListMove`
   - MovableList Set → 写入 `ListSet`
   - Tree → 写入 `RawTreeMove`（引用 peer_idx/position_idx 等）
   - Future → 写入 `I64/F64/Unknown(...)`
4. `len`：必须等于 `op_len(op.content)`（见 4.4.2），用于 change atom_len 的累计。
5. Change header 部分：
   - change atom_len：写入 n-1 个（最后一个由 counter_len - sum 推导）
   - dep_on_self 优化：若 deps 包含 `ID(peer, change_start_counter-1)`，可设 dep_on_self=true 并从 deps 中移除该项再编码其它 deps
   - lamport：写入 n-1 个（最后一个由 lamport_start/lamport_len 推导）

---

## 4.6 测试用 JSON 形态（建议）

为便于跨语言对照，建议 Moon `decode --emit-changes-json` 输出尽量对齐 Rust 的 `encoding/json_schema.rs::json::JsonChange/JsonOp`：

- `Change` JSON：
  - `id`：`"{counter}@{peer}"`
  - `timestamp`：i64
  - `deps`：`["{counter}@{peer}", ...]`
  - `lamport`：u32
  - `msg`：string or null
  - `ops`：数组
- `Op` JSON：
  - `container`：`ContainerIDString`
  - `counter`：i32
  - `content`：按容器类型的 tagged object（如 `{"type":"insert",...}`），字段名与 Rust json_schema 保持一致
  - `fractional_index`：大写 hex string
  - `LoroValue`：按 4.2 的 human-readable 规则

同时建议 Moon 额外提供一个 debug 输出（不参与对照）：

- `wire`：包含 `container_idx/prop/value_type/len` 与 values/delete_start_ids 消费位置（用于排查编码映射错误）

---

## 4.7 建议的测试切入点（利用 Change/Op）

1. **单位测试（decode_op 映射）**：
   - 给定 `(container_type, prop, value_kind+payload, delete_start_id?)`，断言还原的 `OpContent` 正确。
2. **Golden 测试（changes.json 对照）**：
   - Rust 为每个 updates 用例额外输出 `changes.json`（可复用 `encoding::json_schema::export_json_in_id_span` 或定制导出）。
   - Moon decode 同一个 blob 输出 `changes.json`，做结构化 diff（忽略 debug 字段）。
3. **端到端（transcode + import）**：
   - 仍以 Rust import 后 deep value 对比为最终判定，但 Change/Op 层的 diff 可快速定位“错在 ops 还是 state”。
