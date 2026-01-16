# 12. Seq CRDT Spec（List/Text/MovableList 共享：Fugue + Eg-walker 启发）

本文件整理 Loro 在 **List/Text**（以及 MovableList 的 “ListItem 序列”）上使用的序列 CRDT 核心算法规格。该实现受到 *Event Graph Walker*（Diamond Types）启发，并在 Loro 中通过 FugueSpan + Rope/Tracker 的方式落地。

> 真值来源：
> - 插入/删除与并发排序：`crates/loro-internal/src/container/richtext/tracker/crdt_rope.rs`
> - Span 模型（FugueSpan/Status/DeleteTimes）：`crates/loro-internal/src/container/richtext/fugue_span.rs`
> - Tracker（按 vv checkout / diff 的组织方式，可选）：`crates/loro-internal/src/container/richtext/tracker.rs`
> - DeleteSpan（正向/反向删除的编码语义）：`crates/loro-internal/src/container/list/list_op.rs`

---

## 12.1 范围与统一抽象

同一套 seq-crdt 基建服务三类容器：

1. **List**：每个原子是一个 list 元素（len=1）
2. **Text**：每个原子是一个 “entity”（unicode 字符或 style anchor），见 14-richtext-spec
3. **MovableList(list-items)**：底层维护一条 list-item 序列（ForOp index），见 13-movable-list-spec

因此，本 spec 用一个统一的抽象：

- **Atom**：序列中的最小单位（长度=1）
- **Chunk**：插入时一次带来的连续 atoms（长度=N）
  - list：N 个 value
  - text：N 个 entity
  - movable-list：N 个 list-item

**关键点**：CRDT 层只要求 `Chunk.len()` 与可 slice；Chunk 的内容含义由上层容器解释。

---

## 12.2 核心数据模型（FugueSpan）

### 12.2.1 Span（FugueSpan）

一个 span 表示一次插入产生的连续 atoms（可能被 split），字段语义（需与 Rust 对齐）：

- `id : IdFull`  
  span 的起始 op id（包含 `peer/counter/lamport`）
- `real_id : Option<ID>`  
  某些 span 可能是“占位符”（unknown/gc），此时 `id` 不是实际 id；当删除发生时通过 `delete.start_id` 补齐真实 id。
- `status : Status`
  - `future : bool`：是否属于“未来版本”的 span（用于 checkout/diff；MVP 可先不实现）
  - `delete_times : i16`：被删除次数（tombstone 计数）
- `origin_left : Option<ID>`：插入时的左锚点（见 12.3）
- `origin_right : Option<ID>`：插入时的右锚点（见 12.3）
- `content : Chunk`：可 slice、可 merge（同种 chunk）

### 12.2.2 激活判定（is_activated）

一个 span 在当前版本下是否“可见”（activated）：

```
activated = (status.delete_times == 0) && (!status.future)
```

Rust 真值：`Status::is_activated()`。

---

## 12.3 插入：origin_left/origin_right 与并发排序

### 12.3.1 概念：active index

CRDT rope 对外接受的位置 `pos` 是 **active index**：

- 仅统计 `activated` 的 atoms
- tombstone（delete_times>0）与 future（future=true）不计入 active length

Rust 真值：`ActiveLenQueryPreferLeft/Right` 查询使用的 cache 即 active length。

### 12.3.2 计算 origin_left / origin_right

当在 active index `pos` 插入新 span 时：

- `origin_left`：`pos-1` 处的“活着”的原子 id（若 pos==0 则 None）
- `origin_right`：从 `pos` 开始向右扫描，找到第一个 **non-future** 的原子 id（可能是 tombstone，但必须 `future=false`）；若不存在则 None

> 注意：这里刻意忽略 future spans（用于 checkout/diff 期间的稳定插入）。

Rust 真值：`CrdtRope::insert` 中对 `origin_left/origin_right` 的计算。

### 12.3.3 parent_right（右父节点）概念

插入排序需要一个额外概念：`parent_right_leaf`（右父节点在 rope 中的位置）。

规则（与 Rust 一致）：

- 若 `origin_right` 是“第一个 non-future 节点”的起点，并且它的 `origin_left == 当前 insertion 的 origin_left`，则 `parent_right_leaf = Some(leaf(origin_right))`
- 否则 `parent_right_leaf = None`

用途：当多个并发插入共享同一个 `origin_left` 但 `origin_right` 不同，需要用其 right-parent 的相对位置决定顺序。

---

## 12.4 CrdtRope::insert 规范（必须可逐行 port）

### 12.4.1 输入/输出

输入：

- `pos : usize`（active index）
- `content : FugueSpan`（其 `id/content` 已设置，`origin_left/right` 在函数内计算并写入）
- `find_elem(id: ID) -> LeafIndex`（通过 `origin_right` 找到其所在 leaf）

输出：

- 返回插入后新 span 所在 leaf，以及由于 split 产生的新 leaf 列表（用于更新 id→cursor 映射）

### 12.4.2 算法要点（按 Rust 逻辑描述）

1. 用 `ActiveLenQueryPreferLeft(pos)` 找到插入起点 cursor `start.cursor`  
   - 注意：cursor 可能落在一个 `rle_len==0` 的节点上（需按 Rust 逻辑处理）
2. 计算 `origin_left` 与 `origin_right`，并将其写入 `content.origin_left/right`
3. 收集 `in_between`：
   - 从 `start.cursor` 向右迭代
   - 把连续的 `future=true` 的 spans 收集进 `in_between`
   - 一旦遇到 `future=false` 就停止（该节点贡献 `origin_right`）
4. 初始化 `insert_pos = start.cursor`
5. 若 `in_between` 非空，执行“扫描式插入点选择”：
   - 维护 `visited : [IdSpan]`，表示已扫描过的 span 范围
   - 对每个 `other_elem`（future span）：
     - 若 `other_elem.origin_left != content.origin_left` 且其 `origin_left` 不在 `visited` 内：
       - break（说明 other 的 origin_left 在 content 的左侧，content 必须在 other 左侧）
     - 将 other 的 `id_span` 加入 visited
     - 若 `content.origin_left == other.origin_left`：
       - 若 `other.origin_right == content.origin_right`：
         - 若 `other.id.peer > content.id.peer`：break（peer tie-break）
         - else：`scanning=false`
       - 否则（右父不同）：
         - 计算 `other_parent_right_idx`：
           - 若 other.origin_right 存在：取 `find_elem(other.origin_right)` 得到 leaf，并验证该 leaf 起点 id 等于 other.origin_right
           - 仅当 `that_leaf.origin_left == content.origin_left` 时保留该 idx，否则视为 None
         - 比较 `cmp_pos(other_parent_right_idx, parent_right_leaf)`：
           - Less  => `scanning=true`
           - Equal 且 `other.id.peer > content.id.peer` => break
           - 其它 => `scanning=false`
     - 若 `scanning==false`：更新 `insert_pos = Cursor{ leaf: other_leaf, offset: other_elem.len }`
6. 最终在 `insert_pos` 处插入 `content`

> 备注：上述逻辑直接对应 `crdt_rope.rs` 的实现；MoonBit 落地时建议先逐行 port，后续再重构为更可读版本。

---

## 12.5 删除：DeleteSpan 语义与 CrdtRope::delete

### 12.5.1 DeleteSpan（编码语义）

`DeleteSpan` 允许 `len` 为负，用于表示“反向删除”（便于合并）：

- `pos : isize`
- `signed_len : isize`（不可为 0）

定义：

- 若 `signed_len > 0`：删除区间为 `[pos, pos+signed_len)`
- 若 `signed_len < 0`：删除区间为 `(pos+signed_len, pos]`（反向）

辅助函数（必须与 Rust 对齐）：

- `start()` / `end()` / `last()` / `is_reversed()` / `len()`  
  见 `crates/loro-internal/src/container/list/list_op.rs::DeleteSpan`。

同时，删除 op 必须携带 `start_id`（被删目标 span 的**最左** ID）：

- `DeleteSpanWithId { id_start: ID, span: DeleteSpan }`

### 12.5.2 CrdtRope::delete（规范）

输入：

- `start_id : ID`：**目标** deleted span 的最左 ID（用于填充 placeholder span 的 real_id）
- `pos : usize`：active index
- `len : usize`
- `reversed : bool`

行为：

- 若 `reversed && len>1`：
  - 按 `(len-1 .. 0)` 逐个删除 1 个原子（递归调用），并将 `start_id` 增加偏移（`start_id.inc(i)`）
  - 这与 Rust 逻辑一致（当前实现是 O(len) 的逐个删除；后续可优化）
- 否则：
  - 用 `ActiveLenQueryPreferRight(pos)` 找到起点 cursor（偏向右边界）
  - 若删除完全落在同一 leaf 内，则对该 leaf 做 `update_with_split`：
    - 断言待删 span 是 activated
    - 对被删范围内 span：`delete_times += 1`
    - 若 `real_id` 为空，则用当前 `start_id` 填充；然后 `start_id += span_len`
  - 否则，对跨 leaf 的范围做 `tree.update(start..end)`：
    - 对每个 activated span 执行上述 delete_times/real_id 逻辑

输出：

- 返回 split 信息（用于维护 id→cursor 映射）

Rust 真值：`crates/loro-internal/src/container/richtext/tracker/crdt_rope.rs::delete`。

---

## 12.6 id → cursor 映射（IdToCursor，MVP 可简化但需满足功能）

`IdToCursor` 的职责：把 op id 映射到 rope 中的位置/效果，用于：

- checkout/diff（可选）
- 将 move/delete 等非插入操作关联到正确的 span 范围

Rust 的实现包含大量性能优化（fragment/InsertSet）；MoonBit MVP 可以先做功能版：

最低功能要求：

1. 对 insert：能从 `ID(peer,counter)` 找到该原子所在的 `LeafIndex + offset`
2. 对 delete：能记录“该 delete op 删除了哪些 id_span”
3. 对 move（MovableList）：能记录 “move op 从哪个被删 id（fake/placeholder id）移动到哪个 leaf”
4. 当 rope 插入/删除导致 leaf split 时，能更新受影响 id_span 的 leaf 指向

建议落地阶段分两步：

- v1：用 `Map[(peer,counter) -> Cursor]` 的朴素结构（单元测试覆盖 split 更新）
- v2：对齐 Rust 的 fragment/InsertSet（提升性能，减少内存）

Rust 真值参考：`crates/loro-internal/src/container/richtext/tracker/id_to_cursor.rs`。

### 12.6.1 MovableList 的 move_item（对齐 Rust）

MovableList 的 move 在 tracker 层会以“delete + insert MoveAnchor”实现，但需要额外记录 **被删节点的 fake id**：

- 先 `rope.delete(deleted_id, from_pos, 1)`，在回调里拿到被删 span 的 `fake_delete_id = span.id.id()`
- 再 `rope.insert(to_pos, content=MoveAnchor)`（MoveAnchor 必须阻止与其它 span merge）
- 最后 `id_to_cursor.insert(op_id, Cursor::Move { from: fake_delete_id, to: inserted_leaf })`

Rust 真值参考：`crates/loro-internal/src/container/richtext/tracker.rs::move_item`。

---

## 12.7 应用顺序（规范建议）

为了使 `pos`（active index）在所有副本上有一致语义，规范建议对导入的 changes/ops 使用确定性顺序：

- 按 `(lamport, peer, counter)` 升序处理 op 原子
- 同一 change 内按 counter 连续递增（由编码保证）

Rust 解码后会按 lamport 排序 changes（`sort_unstable_by_key(|c| c.lamport)`）；MoonBit 侧建议补齐 peer/counter tie-break 以保证确定性（见 16-import-export-spec）。

---

## 12.8 与上层容器的对接点

### 12.8.1 List

- 插入 chunk len = 插入元素个数
- 删除 len = 删除元素个数
- chunk 内容可只是 “len”，不必包含具体值（值由上层 arena/ops 绑定）

Rust 参考：`diff_calc.rs::ListDiffCalculator` 用 `RichtextChunk::Text(range)` 承载 list 插入的长度。

### 12.8.2 Text

- chunk len = entity 数（unicode 字符 + style anchors）
- chunk 需要同时支持：
  - 文本实体（unicode/utf8 索引换算）
  - 样式锚点实体（Start/End）
详见 14-richtext-spec。

### 12.8.3 MovableList(list-items)

- list-items 的 seq-crdt 只负责“位置记录”的序列顺序与删除
- 元素 identity 与 move/set 的 LWW 逻辑在 13-movable-list-spec 定义

---

## 12.9 MoonBit 落地建议（spec-driven）

实现顺序建议：

1. 先把 `FugueSpan/Status/DeleteSpan` 等结构与测试对齐（对照 Rust 单元测试）
2. 逐行 port `CrdtRope::insert/delete`（先不做 checkout/diff）
3. 再实现最小 `IdToCursor`（功能版）
4. 最后再做性能优化（fragment/InsertSet）
