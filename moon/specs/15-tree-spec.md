# 15. Tree Spec（Movable Tree + FractionalIndex）

本文件定义 Loro 的可移动树（`LoroTree` / `LoroTreeNode`）语义：节点模型、parent/children、顺序（FractionalIndex）、并发决胜（last-move-wins）、cycle 处理、以及 meta map 容器联动。

> 真值来源：
> - Tree state 与 parent/children cache：`crates/loro-internal/src/state/tree_state.rs`
> - Tree op：`crates/loro-internal/src/container/tree/tree_op.rs`
> - FractionalIndex：`crates/fractional_index/`
> - TS/WASM API：`crates/loro-wasm/src/lib.rs::{LoroTree,LoroTreeNode}`

---

## 15.1 基本概念

### 15.1.1 TreeID 与节点

- `TreeID` 与 `ID(peer,counter)` 同构（Loro 以 op id 作为节点 id）
- 一个节点的“存在/删除”由其 parent 链决定（见 15.3）

Rust 真值：`loro_common::TreeID`（本质是 peer+counter）。

### 15.1.2 TreeParentId（parent 语义）

节点 parent 取值：

- `Root`：根节点（parent=None）
- `Node(TreeID)`：普通父子关系
- `Deleted`：删除根（语义上等价于 move 到 `DELETED_TREE_ROOT`）
- `Unexist`：内部用（用于 retreat/uncreate 推断；对外不暴露）

Rust 真值：`TreeParentId` 定义与 `From<Option<TreeID>>` 转换（`tree_state.rs`）。

---

## 15.2 顺序：FractionalIndex 与 NodePosition

### 15.2.1 sibling 排序键

同一 parent 下 children 的顺序按 `NodePosition` 排序：

- 主键：`fractional_index : FractionalIndex`
- 次键：`idlp : IdLp(lamport,peer)`（用于处理 fractional index 相同的极端情况）

Rust 真值：`NodePosition { position, idlp }` 派生 `Ord`（`tree_state.rs`）。

### 15.2.2 FractionalIndex 生成（插入到指定 index）

本地 `create/move` 若指定 `index`，需要生成一个新的 fractional index，使其落在：

- `left = children[index-1].fi`（若 index>0）
- `right = children[index].fi`（若 index<children_len）

基本规则（对齐 Rust）：

- 无 children：返回 `FractionalIndex::default()`
- 否则：`FractionalIndex::new(left?, right?)`

Rust 真值：`NodeChildren::generate_fi_at`（调用 `FractionalIndex::new`）。

### 15.2.3 冲突与重排（Rearrange）

当 `left.fi == right.fi`（或扫描发现多项相同 fi），需要触发“重排”：

1. 收集需要 reset 的一段连续节点（与 left 相同 fi）
2. 调用 `FractionalIndex::generate_n_evenly(left, next_right, n)` 生成 n 个新 fi
3. 返回 `Rearrange([(target_id, fi_new), ...])`，其中包含新节点与被 reset 的旧节点

实现要求：

- runtime 必须将这类重排编码为额外的 move ops（对旧节点更新 position）
- 重排必须是确定性的（对齐 Rust 扫描与生成方式）

Rust 真值：`NodeChildren::generate_fi_at`（`generate_n_evenly` 分支）。

---

## 15.3 并发与删除：last-move-wins + cycle 处理

### 15.3.1 last_move_op（决胜键）

每个节点记录 `last_move_op : IdFull`，用于决定最终 parent/position：

- 新的 move/create/delete 若其 `idlp` 更大，则覆盖旧状态
- 规则与 Map/MovableList 类似：按 `(lamport, peer)` total order 决胜

Rust 真值：tree diff 计算中使用 `last_effective_move_op_id`，并在 `TreeStateNode.last_move_op` 存储。

### 15.3.2 delete 的语义

delete 不是物理删除节点，而是一次 move：

- `parent = Deleted`
- `position = None`

其效果：

- 该节点（及其子树）在 `Root` 视角不可见
- 仍可在 `Deleted` 子树下遍历（内部/调试）

Rust 真值：`TreeOp::Delete` 与 `TreeParentId::Deleted`。

### 15.3.3 cycle 处理

规范要求：

- 本地 API：禁止创建 cycle，若 `target` 是 `parent` 的祖先（或相等）则报错
- 远端导入：若某个 move 会造成 cycle，则该 move **无效**（忽略，不改变 state）

Rust 真值：

- `TreeState::mov(with_check=true)`：检测 `is_ancestor_of` 并返回 `CyclicMoveError`
- `apply_diff` 在 need_check 时对错误 `unwrap_or_default()`（忽略该 move）

### 15.3.4 parent 不存在

- 本地：move/create 到不存在的 parent 应报错（ParentNotFound）
- 远端：若 parent 不存在（理论上不应出现，但可能在 partial history 中），该 move 无效（忽略）

Rust 真值：`TreeState::mov(with_check=true)` 的 parent existence 检查。

---

## 15.4 操作 API → op 映射（编码语义）

Tree 的 oplog op（见 `moon/loro_codec/op.mbt::TreeOp`）：

- `Create(target, parent?, fi)`
- `Move(target, parent?, fi)`
- `Delete(target)`

关键规则：

- `target` 的 `ID` 必须与创建该节点的 op_id 相同（create 判定依赖 `subject == op_id`，见 codec 解码：`is_create = subject == op_id`）
- `fi` 必须是 FractionalIndex 的 bytes（codec positions arena 存 bytes；JSON 展示为 hex）

Rust/Moon 真值参考：

- Moon 解码：`moon/loro_codec/change_block_ops_decode_content.mbt` 对 `is_create` 的判断
- Rust：`crates/loro-internal/src/container/tree/tree_op.rs`

---

## 15.5 meta map（TreeNode.data）

每个 Tree 节点有一个关联的 Map 容器用于存储元数据：

- `node.data : LoroMap`（WASM API getter 名为 `data`）
- 其 container id 为 `Normal(node.peer, node.counter, Map)`（见 11-core-model）

输出：

- `tree.toJSON()` 返回 hierarchy nodes，其中每个节点包含：
  - `id` / `parent` / `index` / `fractional_index`
  - `meta`（或 TS 中通过 `data` 访问）应在 deep 输出中递归展开为 map 的 deep value
  - `children` 递归

Rust 真值：

- 节点 value 结构：`TreeNodeWithChildren::into_value`
- deep 展开 meta：`tree_state.rs::get_meta_value`（把 meta 字段从 ContainerID 替换成 deep value）

---

## 15.6 最小测试断言（用于 17-test-plan）

必须覆盖：

- 并发 move：同一节点不同 parent/index 的 last-move-wins（按 IdLp）
- cycle：并发互相移动造成潜在 cycle，结果必须无 cycle（某个 move 被忽略）
- reorder：在同一 parent 下大量插入导致 fractional index 冲突，触发 rearrange 后顺序稳定
- delete：删除后节点不可见，但其 meta 容器也应从可达集合中消失（若无其它引用）

