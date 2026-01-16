# 13. MovableList Spec（Identity + Move/Set 的并发语义）

MovableList 是 Loro 的“可移动列表”：与普通 List 不同，它为每个元素提供稳定 identity，使 `move` 与 `set` 能在并发场景下避免“delete+insert”导致的冗余元素。

> 真值来源：
> - 核心 diff 语义（pos/value 的 LWW）：`crates/loro-internal/src/diff_calc.rs::MovableListDiffCalculator`
> - 状态结构（list-items + elements）：`crates/loro-internal/src/state/movable_list_state.rs`
> - Move 在 seq-tracker 中的表示：`crates/loro-internal/src/container/richtext/tracker.rs::move_item`
> - TS/WASM API：`crates/loro-wasm/src/lib.rs::LoroMovableList`

---

## 13.1 模型总览

MovableList 可以理解为两层结构：

1. **ListItem 序列（CRDT 序列）**：表示“位置记录”，决定顺序与删除（使用 12-seq-crdt）
2. **Element 表（identity → value/pos）**：表示“元素实体”，可被 move/set 更新（使用 LWW 决胜）

对用户而言：

- `insert` 创建新 element（identity 固定）
- `move` 仅改变 element 的位置（identity 不变）
- `set` 仅改变 element 的值（identity 不变）

---

## 13.2 核心类型与索引空间

### 13.2.1 elem_id（identity）

- `elem_id : IdLp(peer, lamport)`  
  用于唯一标识元素（跨移动保持不变），并作为 move/set 的引用目标。

Rust 真值：`loro_common::IdLp`（Ord by lamport then peer）。

### 13.2.2 两套索引（必须区分）

MovableList 同时存在两套索引：

- **User index（ForUser）**：只统计“当前可见”的元素
- **Op index（ForOp）**：统计底层 list-items（包括被 move 走后不再可见的记录/或 move 的中间记录）

规范要求：

- 对外 API（`get/insert/delete/move/set`）使用 **User index**
- 编码到 oplog 的 op（`Insert/Delete/Move`）内部字段 `pos/from/to` 使用 **Op index**
- runtime 在生成本地 op 时必须做 User↔Op 的转换（依赖当前状态）

Rust 真值：

- state 内部用 `IndexType::{ForUser,ForOp}`（`movable_list_state.rs`）
- WASM `move/set` API 接受 user index，但底层 handler 会转为 op index

---

## 13.3 操作语义（与编码字段对齐）

本节以 `moon/loro_codec/op.mbt::MovableListOp` 为准：

- `Insert(pos, values[])`
- `Delete(pos, len, start_id)`
- `Move(from, to, elem_id)`
- `Set(elem_id, value)`

### 13.3.1 Insert（创建 identity）

语义：

- 在 **Op index** `pos` 处插入 `N=values.length` 个 list-item
- 同时创建 `N` 个新的 element identity（elem_id）

规范 identity 分配（对齐 Rust）：

对一次 insert op（其起始 op_id_full 为 `(peer,counter,lamport)`，len=N）：

- 第 `i` 个元素的：
  - `elem_id = IdLp(peer, lamport+i)`
  - `value_id = elem_id`（初始值的 LWW 版本号）
  - `pos_id = IdLp(peer, lamport+i)`（初始位置版本号；与对应 list-item 的 idlp 一致）

Rust 真值：

- `MovableListDiffCalculator::apply_change`：insert 时 `id = op.id_full().idlp().inc(i)`
- state local apply：`movable_list_state.rs::apply_local_op` 同样按 `op.idlp().inc(i)` 生成 elem_id

### 13.3.2 Delete（删除 list-items 与 elements）

Delete 的编码形式沿用 `DeleteSpanWithId`（见 12-seq-crdt）：

- `pos/len` 表示要删除的 list-item 范围（**Op index**）
- `start_id` 用于修正 placeholder span 的 real_id（与 seq-crdt 一致）

语义：

1. 在 list-items 序列中删除该范围（seq-crdt delete）
2. 对每个被删 list-item，若其 `pointed_by = Some(elem_id)`：
   - 删除该 element（因此用户视角长度减少）
   - 若 element.value 是容器引用，需要记录为“可能被删除的 child 容器”（用于容器可达性更新）

### 13.3.3 Move（移动 identity）

Move op 字段：

- `from : OpIndex`
- `to : OpIndex`
- `elem_id : IdLp`

语义（概念上）：

1. 在 `from` 位置删除一个 list-item（它应当指向 elem_id）
2. 在 `to` 位置插入一个新的 list-item（表示 elem_id 的新位置）
3. 更新 element 的 `pos_id`（LWW 决胜），并让新 list-item `pointed_by = elem_id`

并发决胜（规范）：

- 对同一 `elem_id` 的多个 move（含初始 insert 的 pos）：
  - 取 `pos_id` 最大者（按 `IdLp(lamport,peer)`）作为最终位置
  - 较小的 move 可能仍会留下“中间 list-item 记录”，但不应在用户视角可见（`pointed_by=None`）

Rust 真值（LWW 决胜）：

- `MovableListDiffCalculator::apply_change`：`change.pos = max(change.pos, idlp_of_move_op)`

seq-tracker 层的 move 表示（用于 checkout/diff，可选但建议对齐）：

- `Tracker::move_item(op_id_full, deleted_id, from_pos, to_pos)`：
  - 内部执行 `rope.delete(deleted_id, from_pos, 1)` + `rope.insert(to_pos, MoveAnchor)`
  - 并在 `IdToCursor` 记录 `Cursor::Move { from: fake_deleted_id, to: inserted_leaf }`

Rust 真值：`crates/loro-internal/src/container/richtext/tracker.rs::move_item`。

### 13.3.4 Set（原地更新值）

Set op 字段：

- `elem_id : IdLp`
- `value : LoroValue`

语义：

- 更新 element 的 `value`，并设置 `value_id = IdLp(peer_of_op, lamport_of_op)`
- 并发决胜：对同一 elem_id 的多个 set，取最大 `value_id` 的 value

Rust 真值：

- `MovableListDiffCalculator::apply_change`：`value_id = max(value_id, idlp)`。

---

## 13.4 可观察行为（User 视角）

### 13.4.1 length / get

- `length` 统计当前 `pointed_by != None` 的 list-item 数（可见元素数）
- `get(i)` 返回第 i 个可见元素的 value（若 value 为容器引用，则返回容器 handle；`toJSON` 再深展开）

### 13.4.2 move 的用户语义（与 TS 文档一致）

`move(from,to)`：

- 把 `from` 位置的元素移动到 `to` 位置
- 与 `delete(from,1) + insert(to, value)` 不同，它不会引入并发冗余元素

WASM 文档真值：`LoroMovableList.move` 的注释（避免并发冗余）。

---

## 13.5 容器子引用（child containers）

当 element 的 value 是容器引用时，需要支持：

- `contains_child_container(container_id)`：用于路径查询/可达性
- `get_child_index(container_id)`：返回该 child 在用户序列中的 index（ForUser）

Rust 真值：`MovableListState` 内部 `child_container_to_elem` + `get_child_index`。

---

## 13.6 JSON 输出

- `getShallowValue()`：返回 `Value[]`，其中子容器以 `ContainerID` 字符串表示
- `toJSON()`：返回递归 deep JSON（子容器展开为其 deep 值）

TS/WASM 示例真值：`LoroMovableList.getShallowValue` 注释与 `toJSON`。

