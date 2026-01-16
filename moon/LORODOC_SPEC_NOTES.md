# MoonBit LoroDoc Runtime – SPEC NOTES

本文件记录实现 LoroDoc runtime（`moon/loro_doc/`）时最关键、最容易踩坑的规格要点，作为 `moon/specs/09-lorodoc-spec-pack.md` 的“实现速查表”。

真值以 Rust 为准（见每条后面的 Rust 指针）。

---

## 1) ID / Lamport / vv（必须对齐）

- `ID = (peer:u64, counter:i32)`，`VersionVector[peer] = next_counter (exclusive)`  
  Rust：`crates/loro-internal/src/version.rs`
- `IdLp = (lamport:u32, peer:u64)`，总序为 `(lamport, peer)`（字段顺序即比较顺序）  
  Rust：`crates/loro-common/src/lib.rs::IdLp`
- 新 change 的 `start_lamport = max(lamport_of(dep) + 1)`（deps=frontiers），deps 为空则 0  
  Rust：`crates/loro-internal/src/oplog/loro_dag.rs::frontiers_to_next_lamport`
- change 内的 op 原子必须让 counter/lamport 都连续递增（len=N 则 +N）  
  Rust：`crates/loro-internal/src/txn.rs`

对应 spec：`moon/specs/11-lorodoc-core-model-spec.md`。

---

## 2) Root 容器与 ContainerID

- root name 校验：非空，且不含 `/` 与 `\\0`  
  Rust：`crates/loro-common/src/lib.rs::check_root_container_name`
- root 容器逻辑上永远存在（`has_container(root)=true`）  
  Rust：`crates/loro-internal/src/loro.rs::has_container`
- 子容器（Normal）ID 必须由“创建它的 op_id + 容器类型”推导  
  Rust：容器值编码依赖该不变量；Moon codec 侧亦有对应限制说明（`moon/SPEC_NOTES.md`）
- Tree node 的 meta map：`cid = Normal(node.peer,node.counter,Map)`  
  Rust：`TreeID::associated_meta_container()`（tree handler/state 中使用）

对应 spec：`moon/specs/11-lorodoc-core-model-spec.md`、`moon/specs/10-lorodoc-api-spec.md`。

---

## 3) Seq CRDT（List/Text/MovableList 共用）

- activated 判定：`delete_times==0 && !future`  
  Rust：`crates/loro-internal/src/container/richtext/fugue_span.rs::Status`
- 插入排序核心逻辑必须逐行对齐 `CrdtRope::insert`（origin_left/right + in_between 扫描 + peer tie-break）  
  Rust：`crates/loro-internal/src/container/richtext/tracker/crdt_rope.rs`
- 删除需要支持 reverse delete（DeleteSpan.signed_len<0）  
  Rust：`crates/loro-internal/src/container/list/list_op.rs::DeleteSpan`
- MovableList 的 move 在 tracker 层是 `delete(1) + insert(MoveAnchor)`，并在 `IdToCursor` 记录 `Cursor::Move`  
  Rust：`crates/loro-internal/src/container/richtext/tracker.rs::move_item`

对应 spec：`moon/specs/12-seq-crdt-spec.md`。

---

## 4) MovableList（identity + LWW）

- op 字段的 `pos/from/to` 是 **ForOp index**（不是用户 index），API 层必须做 User↔Op 转换  
  Rust：`movable_list_state.rs` 的 `IndexType::{ForUser,ForOp}` 与 wasm API
- element 的 pos/value 都是 LWW by `IdLp(lamport,peer)`  
  Rust：`crates/loro-internal/src/diff_calc.rs::MovableListDiffCalculator`

对应 spec：`moon/specs/13-movable-list-spec.md`。

---

## 5) RichText（样式 anchors + expand）

- oplog 持久化位置用 **Entity index**（unicode + anchors），不是 unicode index  
  Rust：`crates/loro-internal/src/container/richtext.rs` 文件头注释
- StyleStart/End 必须配对且相邻；expand 由 `TextStyleInfoFlag` 决定插入相对 anchor 的 side  
  Rust：`TextStyleInfoFlag::prefer_insert_before`（`richtext.rs`）、`StyleConfigMap`（`config.rs`）
- unicode/utf8/utf16 的边界换算必须与 Rust 对齐（否则 len/pos 会错）  
  Rust：`crates/loro-internal/src/container/richtext/richtext_state.rs`

对应 spec：`moon/specs/14-richtext-spec.md`。

---

## 6) Tree（FractionalIndex + cycle）

- sibling 排序：`(fractional_index, idlp)`，用于处理 fi 相同的极端情况  
  Rust：`NodePosition`（`tree_state.rs`）
- fi 生成：`FractionalIndex::new(left?,right?)`；冲突时 `generate_n_evenly` 并触发 rearrange（需要额外 move ops）  
  Rust：`NodeChildren::generate_fi_at`（`tree_state.rs`）
- cycle：本地报错；远端导入时产生 cycle 的 move 无效（忽略）  
  Rust：`TreeState::mov(with_check)` + `apply_diff` 的忽略逻辑

对应 spec：`moon/specs/15-tree-spec.md`。

---

## 7) Import / Export updates（mode=4）

- import：解析 document(mode=4) → blocks → decode_change_block → changes；按确定性顺序应用（建议 `(lamport,peer,counter)`）  
  Rust：`fast_snapshot::decode_updates`（排序仅按 lamport）
- export：从 vv 增量导出 changes，并编码为 change blocks（可先“一 peer 一 block”），再 encode_document(mode=4)  
  Rust：`fast_snapshot::encode_updates`（最终 `oplog.export_blocks_from(vv)`）

对应 spec：`moon/specs/16-lorodoc-import-export-spec.md`。

