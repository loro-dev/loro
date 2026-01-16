# 10. LoroDoc Runtime API Spec（MoonBit）

本文件定义 MoonBit 侧 `LoroDoc`（运行时 CRDT 引擎）的**对外 API 规格**，目标是提供与 `loro-crdt`（TS/WASM）一致的用户心智模型（非 deprecated/outdated）。

> 真值来源：`crates/loro-wasm/src/lib.rs`（TS/WASM 导出 API）与 `crates/loro-internal/src/handler.rs`（Rust handler 行为）。

---

## 10.1 范围

### 必须覆盖

- `LoroDoc`：创建、peer、commit/import/export、版本查询、`toJSON/getShallowValue`
- `Container` 基类（kind/id/isAttached/isDeleted/getShallowValue/toJSON）
- `LoroMap` / `LoroList` / `LoroMovableList` / `LoroText` / `LoroTree` / `LoroTreeNode`
- 容器插入/设置子容器：`setContainer/insertContainer/pushContainer/setContainer`

### 本 spec 不要求实现（但可预留 API）

- subscribe/事件系统、UndoManager、Awareness/Ephemeral、detached editing / checkout / time-travel

---

## 10.2 公共类型（对外）

> MoonBit 侧建议尽量复用 `moon/loro_codec/*` 的基础类型定义（`ID/ContainerID/ContainerType/VersionVector` 等）。

### 10.2.1 标识与版本

- `type PeerID = UInt64`
- `type Counter = Int`（语义与 Rust `i32` 一致）
- `type Lamport = UInt`（语义与 Rust `u32` 一致）

- `struct ID { peer: PeerID, counter: Counter }`
  - 与 `moon/loro_codec/id.mbt` 对齐
- `struct IdLp { peer: PeerID, lamport: Lamport }`
  - 与 `moon/loro_codec/op.mbt::IdLp` 对齐

- `type VersionVector = Array[(PeerID, Counter)]`
  - 语义：`vv[peer] = next_counter`（**exclusive**，即“该 peer 已包含的最大 counter + 1”）
  - 与 `moon/loro_codec/postcard_vv_frontiers.mbt` 对齐

### 10.2.2 容器

- `enum ContainerType = Map | List | MovableList | Text | Tree | Counter | Unknown(UInt)`
  - runtime 本计划只要求 Map/List/MovableList/Text/Tree；Counter 可先保留为 Unknown/NotSupported
- `enum ContainerID = Root(name:String, kind:ContainerType) | Normal(peer:PeerID, counter:Counter, kind:ContainerType)`
  - 与 `moon/loro_codec/container_id_bytes.mbt` 对齐

### 10.2.3 值类型（Value）

对外值类型遵循 TS 的 `Value | Container` 心智模型：

- `Value`：JSON-like 值（null/bool/number/string/binary/list/map）+ **容器引用**
- `Container`：运行时容器句柄（Map/List/MovableList/Text/Tree 的任一）

规范要求：

1. 所有容器写入（Map.set / List.insert / …）接受 **Value 或 Container**
2. `toJSON()` 返回 **深展开**（递归把容器转换为其 deep JSON）
3. `getShallowValue()` 返回 **浅展开**（容器引用表现为 `ContainerID` 字符串）

> Rust 真值参考：`crates/loro-internal/src/state.rs::{get_deep_value,get_value,get_container_deep_value}`。

---

## 10.3 LoroDoc API

### 10.3.1 构造与 peer

- `LoroDoc::new() -> LoroDoc`
  - 创建文档，生成随机 `peerId`
  - 默认开启“自动事务”（见 11-core-model）
- `LoroDoc::peerId(self) -> PeerID`
- `LoroDoc::setPeerId(self, peer: PeerID) -> Unit raise LoroError`
  - 风险：调用方必须保证 peer 全局唯一，否则一致性不可保证

Rust 真值参考：

- `crates/loro-wasm/src/lib.rs`：`peerId/setPeerId`
- `crates/loro-internal/src/loro.rs`：`set_peer_id`

### 10.3.2 Root 容器获取

- `getMap(name:String) -> LoroMap`
- `getList(name:String) -> LoroList`
- `getMovableList(name:String) -> LoroMovableList`
- `getText(name:String) -> LoroText`
- `getTree(name:String) -> LoroTree`

规则：

- `name` 必须通过 root name 校验（见 11-core-model：禁止空字符串，禁止包含 `/` 与 `\0`）
- Root 容器 **逻辑上始终存在**；`getX` 只创建/返回 handle（不需要先 “create”）

Rust 真值参考：`crates/loro-internal/src/loro.rs::has_container`（root always true）。

### 10.3.3 提交与版本

- `commit(options?: CommitOptions) -> Unit`
  - 显式提交 pending txn（语义见 11-core-model）
- `oplogVersion(self) -> VersionVector`
  - 返回当前 OpLog 的 vv（用于增量 export）

`CommitOptions`（与 TS 一致）：

- `origin?: String`（仅用于事件标记，不持久化；本阶段可忽略）
- `timestamp?: Int64`（Unix seconds；要求单调不减）
- `message?: String`（commit message，持久化）

### 10.3.4 Import / Export

- `import(bytes: Bytes, origin?: String) -> ImportStatus raise LoroError`
  - 本阶段只要求支持 FastUpdates（mode=4）
- `export(opts: ExportOptions) -> Bytes raise LoroError`
  - 本阶段只要求支持 `{ mode: "update", from?: VersionVector }`

`ExportOptions`：

- `mode: "update"`（MVP）
- `from?: VersionVector`（可选；不传表示从空 vv 导出全量 updates）

`ImportStatus`：

- `success: Bool`
- `pending?: Array[ID]`（或更丰富结构；MVP 可先返回是否存在 pending）

> Import/Export 的语义与编码细节见 `moon/specs/16-lorodoc-import-export-spec.md`。

### 10.3.5 JSON 输出

- `toJSON(self) -> Json`：等价于 Rust `get_deep_value().to_json_value()`
- `getShallowValue(self) -> Json`：等价于 Rust `get_value().to_json_value()`

说明：

- shallow：根 map 的 value 是容器引用（`ContainerID` 字符串）
- deep：根 map 的 value 是容器的 deep 值（递归展开）

---

## 10.4 Container 基类 API

所有容器句柄都必须支持：

- `kind(self) -> ContainerType`（或返回字符串 `"Map"|"List"|...`，实现时任选一种，但需稳定）
- `id(self) -> ContainerID`
- `isAttached(self) -> Bool`
  - root 容器始终 attached
  - 子容器在被插入到 attached 容器后 attached
- `isDeleted(self) -> Bool`
  - 语义：该容器在当前版本下被删除/不可达（见 11-core-model）
- `getShallowValue(self) -> Json`
- `toJSON(self) -> Json`

> TS/WASM 参考：`crates/loro-wasm/src/lib.rs` 中各容器 `kind/id/isDeleted/getShallowValue/toJSON`。

---

## 10.5 LoroMap API

### 基本操作

- `get(key:String) -> Value?`
- `set(key:String, value: ValueOrContainer) -> Unit`
- `delete(key:String) -> Unit`
- `has(key:String) -> Bool`
- `keys() -> Array[String]`
- `entries() -> Array[(String, ValueOrContainer)]`（浅，不递归）

### 子容器

- `setContainer(key:String, child: Container) -> Container`
  - `child` 可为 detached 容器实例（`new LoroText()` 等）
  - 返回 attached 的 child handle（其 `id` 基于本次 op_id 生成；规则见 11-core-model）

---

## 10.6 LoroList API

### 基本操作

- `length(self) -> Int`
- `get(index:Int) -> ValueOrContainer?`
- `insert(index:Int, value: ValueOrContainer) -> Unit`
- `delete(index:Int, len:Int) -> Unit`
- `push(value: ValueOrContainer) -> Unit`
- `toArray() -> Array[ValueOrContainer]`（浅，不递归）

### 子容器

- `insertContainer(index:Int, child: Container) -> Container`
- `pushContainer(child: Container) -> Container`

---

## 10.7 LoroMovableList API

> MovableList 的索引空间与 CRDT 语义见 `moon/specs/13-movable-list-spec.md`。

### 基本操作

- `length(self) -> Int`（用户可见长度）
- `get(index:Int) -> ValueOrContainer?`
- `insert(index:Int, value: ValueOrContainer) -> Unit`
- `delete(index:Int, len:Int) -> Unit`
- `push(value: ValueOrContainer) -> Unit`

### 移动与原地修改

- `move(from:Int, to:Int) -> Unit`
- `set(index:Int, value: ValueOrContainer) -> Unit`
- `setContainer(index:Int, child: Container) -> Container`

---

## 10.8 LoroText API（RichText）

> 详细语义见 `moon/specs/14-richtext-spec.md`。

- `length(self) -> Int`（unicode scalar count）
- `toString(self) -> String`
- `insert(index:Int, text:String) -> Unit`
- `delete(index:Int, len:Int) -> Unit`

样式：

- `mark(range:{start:Int,end:Int}, key:String, value: Value, expand?: ExpandType) -> Unit`
  - expand 由 doc 侧 style config 决定；若显式传入则覆盖（可选项）
- `unmark(range, key:String) -> Unit`（等价于写入“删除样式”的 mark 事件）
- `toDelta() -> Array[DeltaSpan]`
  - `DeltaSpan = { insert: String, attributes?: Map[String, Value] }`

Doc 侧配置：

- `LoroDoc::configTextStyle(map: Map[String, { expand: "before"|"after"|"both"|"none" }])`
- `LoroDoc::configDefaultTextStyle(style?: { expand: ... })`

---

## 10.9 LoroTree / LoroTreeNode API

> 详细语义见 `moon/specs/15-tree-spec.md`。

### LoroTree

- `createNode(parent?: TreeID, index?: Int) -> LoroTreeNode`
- `move(target: TreeID, parent?: TreeID, index?: Int) -> Unit`
- `delete(target: TreeID) -> Unit`
- `getNodeByID(target: TreeID) -> LoroTreeNode?`
- `toJSON() -> Json`（深展开：children 递归，且 meta/data map 递归 deep）
- `getShallowValue() -> Json`（meta/data 用 ContainerID 表示）

### LoroTreeNode

- `id(self) -> TreeID`
- `parent(self) -> TreeID?`
- `index(self) -> Int?`
- `fractionalIndex(self) -> String?`（hex string）
- `data(self) -> LoroMap`（该节点的 meta map 容器）
- `createNode(index?:Int) -> LoroTreeNode`（以当前节点为 parent）
- `move(parent?:LoroTreeNode, index?:Int) -> Unit`
- `delete() -> Unit`
- `toJSON() -> Json`

