# 11. LoroDoc Runtime – Core Model Spec（ID / Lamport / Change / DAG）

本文件定义 LoroDoc 运行时的核心语义模型：ID、Lamport、Change DAG、版本向量、容器 ID 规则，以及“本地 commit / 远端 import / 增量 export”的最小一致性要求。

> 真值来源：
> - 事务与 lamport 分配：`crates/loro-internal/src/txn.rs`
> - Change 结构与不变量：`crates/loro-internal/src/change.rs`
> - DAG/frontiers/next_lamport：`crates/loro-internal/src/oplog/loro_dag.rs`
> - root name 校验：`crates/loro-common/src/lib.rs::check_root_container_name`

---

## 11.1 基本类型

### 11.1.1 ID / IdFull / IdLp

- `ID = (peer: u64, counter: i32)`：同一 peer 内 counter 单调递增（从 0 开始）
- `Lamport = u32`：全局逻辑时钟，用于 total order（用于 LWW/last-move-wins 等决胜）
- `IdFull = (peer, counter, lamport)`：每个 op 的完整标识
- `IdLp = (lamport, peer)`：按 `(lamport, peer)` 排序的 total order key（Rust 派生 `Ord`）

排序规则（规范）：

- `IdLp` 比较：先 `lamport`，再 `peer`
- `IdFull` 的总序：先 `lamport`，再 `peer`（counter 主要用于定位同 peer 内的 op/span）

Rust 真值参考：`crates/loro-common/src/lib.rs::IdLp`（字段顺序决定 Ord）。

### 11.1.2 VersionVector（vv）

`vv : Map<peer, next_counter>`，语义为：

- `vv.includes_id(ID(peer,c)) == (vv[peer] > c)`
- 初始 `vv` 为空（等价所有 peer 的 next_counter=0）

> 注意：vv 存的是 **exclusive** 的 next counter，而不是 last counter。

Rust 真值参考：`crates/loro-internal/src/version.rs`（includes_id/includes_vv 语义）。

### 11.1.3 Frontiers

`frontiers` 是一组 `ID`，表示 DAG 的“头结点”（每条并发分支的末端）。

最小要求：

- `frontiers` 可以为空（表示空文档）
- 对于一个完整历史（非 shallow），`frontiers` 等价于 `vv_to_frontiers(vv)` 的 shrink 结果

Rust 真值参考：`crates/loro-internal/src/oplog/loro_dag.rs::vv_to_frontiers`。

---

## 11.2 Change 与不变量

### 11.2.1 Change 结构

一条 Change 表示一次 commit（可能包含多个 op，且同一 peer 的 counter 连续）：

- `change.id : ID`：该 change 的起始 op id（first op）
- `change.atom_len : usize`：该 change 覆盖的 op 原子长度（counter/lamport 都连续递增）
- `change.deps : Frontiers`：依赖的变更集合（指向其它 change 的末端）
- `change.lamport : Lamport`：该 change 的第一个 op 的 lamport
- `change.timestamp : i64`：Unix seconds（可选自动记录）
- `change.msg : Option<String>`
- `change.ops : [Op]`

### 11.2.2 关键不变量（必须满足）

1. **deps 只能指向其它 change 的末端**  
   即 dep 的 `ID` 必须等于某个 change 的 `id_last()`（该 change 最后一个 op 的 ID）。  
   Rust 说明：`crates/loro-internal/src/change.rs` 顶部注释。
2. **同一 change 内 counter 连续**  
   `change.id.counter .. change.id.counter + atom_len` 覆盖该 change 的所有 op 原子位置。
3. **同一 change 内 lamport 连续**  
   `change.lamport .. change.lamport + atom_len` 覆盖该 change 的所有 op 原子位置。
4. **lamport 必须满足因果单调**  
   `change.lamport > max(lamport_of(dep))`（dep 是 deps 中的每个 ID）

---

## 11.3 lamport 分配规则（本地 commit）

本地生成新 change 时，必须按 Rust 的规则计算起始 lamport（否则会影响 LWW/Tree/MovableList 的并发决胜）。

### 11.3.1 lamport_of_id(dep_id)

给定一个 dep `ID(peer,counter)`，其对应的 op lamport 计算为：

1. 找到包含该 ID 的 change `C`（同 peer，且 `C.id.counter <= counter < C.id.counter + C.atom_len`）
2. `lamport_of(dep_id) = C.lamport + (counter - C.id.counter)`

### 11.3.2 next_lamport(frontiers)

新 change 的起始 lamport 定义为：

```
if frontiers is empty:
  next_lamport = 0
else:
  next_lamport = max(lamport_of_id(dep) + 1 for dep in frontiers)
```

Rust 真值参考：`crates/loro-internal/src/oplog/loro_dag.rs::frontiers_to_next_lamport`。

### 11.3.3 同一事务内的 lamport/counter 递增

事务生成的 op 可能是“带长度”的（例如 list/text insert 一次插入 N 个原子），规范要求：

- 事务起点：`start_counter = oplog.next_id(peer).counter`
- 对于每个 op 内容长度 `len`：
  - 使用当前 `(next_counter,next_lamport)` 作为该 op 的 `(counter_start, lamport_start)`
  - 然后 `next_counter += len`，`next_lamport += len`

Rust 真值参考：`crates/loro-internal/src/txn.rs`（`next_counter/next_lamport` 递增逻辑）。

---

## 11.4 ContainerID 规则（Root / Normal / meta 容器）

### 11.4.1 Root container name 校验

root name 必须满足：

- 非空
- 不包含 `/`
- 不包含 `\0`

Rust 真值参考：`crates/loro-common/src/lib.rs::check_root_container_name`。

### 11.4.2 Root ContainerID

`ContainerID::Root(name, kind)` 的字符串形式（与 TS 兼容）：

- `cid:root-{name}:{Kind}`

其中 `{Kind}` 为 `Map/List/MovableList/Text/Tree/(Counter)`。

### 11.4.3 Normal ContainerID（子容器）

子容器 ID 必须由“创建它的 op_id + 容器类型”确定：

- `ContainerID::Normal(peer=op_id.peer, counter=op_id.counter, kind=child.kind)`

原因：二进制编码的容器值只存 type，不存完整 id；id 由 op_id 推导（见 `moon/SPEC_NOTES.md` 中 JsonSchema import 限制说明）。

### 11.4.4 Tree node 的 meta map 容器

Tree 的每个节点 `TreeID(peer,counter)` 都有一个关联的 meta map 容器：

- `meta_container_id = ContainerID::Normal(peer, counter, Map)`

Rust 真值参考：`TreeID::associated_meta_container()`（在 tree handler/state 中使用）。

---

## 11.5 “存在性”与删除语义

### 11.5.1 root 容器

- 逻辑上永远存在（即使为空）
- 可配置“隐藏空 root 容器”（Rust 有 `hide_empty_root_containers`；Moon MVP 可先不做）

Rust 真值参考：`crates/loro-internal/src/loro.rs::has_container`、`crates/loro-internal/src/state.rs::get_deep_value`。

### 11.5.2 非 root 容器

- 非 root 容器是否存在/是否 deleted，取决于当前版本下是否仍被父容器引用（可达性）
- `toJSON/get_deep_value` 递归展开容器值时，按当前 state 解析 container_id 到 container_idx，再取 deep value（不做 cycle 检测；cycle 行为未定义）

Rust 真值参考：`crates/loro-internal/src/state.rs::get_container_deep_value`。

---

## 11.6 本地事务与 commit（最小语义）

为对齐 TS/WASM 行为，规范建议实现“自动事务 + barrier 提交”：

- 文档维持一个 pending txn（累计本地操作）
- 显式 `commit()`：
  - 若 txn 为空：不产生 change；commit options 不应 carry 到下次
  - 若 txn 非空：生成 change 并写入 oplog
- 隐式 barrier（至少）：
  - `export(...)`：先隐式 commit（即使 txn 为空也形成 barrier）
  - `import(...)`：先隐式 commit，再导入远端 changes（并更新 state）

Rust 真值参考：`crates/loro-wasm/src/lib.rs::commit` 注释（empty commit 行为差异），以及 `crates/loro-internal/src/loro.rs` 的 `with_barrier/implicit_commit_then_stop`。

> Moon MVP 可先实现“每次 mutating API 立即 commit”为简化版本，但必须在 spec 中标注与 TS 的差异；推荐按上面规则实现以减少后续返工。

