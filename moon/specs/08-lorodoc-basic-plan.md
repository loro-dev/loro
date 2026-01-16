# 08. MoonBit LoroDoc 基础支持计划（Map / List / Text / Tree）

本计划面向 `moon/`：在已具备 Loro **codec primitives** 的前提下，补齐 MoonBit 侧的 **LoroDoc 运行时（CRDT 引擎）**，并提供与 `loro-crdt`（TS 版）相近的使用体验。

> 重点难点：RichText / List / Tree 的 CRDT 算法（Loro 参考了 Event Graph Walker 思路，并在 Text 侧结合 Fugue + tracker/rope 结构实现）。

---

## 1. 目标与范围

### 1.1 必须支持（MVP）

- `LoroDoc::new()`：创建文档（本地 peer、时钟/lamport、oplog、state 等最小闭环）。
- Root 容器：`doc.getMap(name)` / `doc.getList(name)` / `doc.getMovableList(name)` / `doc.getText(name)` / `doc.getTree(name)`。
- 容器操作（与 TS 类似的直觉 API）：
  - **Map**：`get/set/delete/keys/toJSON`
  - **List**：`len/get/insert/delete/push/toJSON`
  - **MovableList**：`len/get/insert/delete/move/set/toJSON`（元素具备稳定 identity，可移动）
  - **Text（RichText）**：`toString/insert/delete/mark/unmark(or markEnd)/toDelta(or toJSON)`
  - **Tree**：`create/move/delete/children/parent/meta(map)/toJSON`
- **Import / Export updates**：可与 Rust/TS 互通（至少支持 update 流；snapshot 可作为下一阶段）。
- **Container 基础能力**：统一容器 handle（`Container.kind/id/doc`），以及 `LoroValue::Container` 的运行时展开/索引（Map/List/MovableList 中嵌套容器）。

### 1.2 不做（明确排除）

- deprecated / outdated API（例如 TS 侧 `class Loro extends LoroDoc` 这种别名，或明确标注 deprecated 的模块）。
- Awareness/Ephemeral/UndoManager 等高级能力（后续另开计划）。
- Counter 容器（本计划先不纳入；后续可按 `feature=counter` 独立扩展）。
- Time-travel/checkout 任意历史版本（可先只维护“当前版本”正确性；后续再补 diff/checkout）。

---

## 2. 对外 API 设计（对齐 loro-crdt TS 心智模型）

> 以 MoonBit 语法表达，具体命名可在落地阶段再统一（camelCase vs snake_case）。

### 2.1 LoroDoc

- `pub fn LoroDoc::new() -> LoroDoc`
- `pub fn LoroDoc::getMap(self, name : String) -> LoroMap`
- `pub fn LoroDoc::getList(self, name : String) -> LoroList`
- `pub fn LoroDoc::getMovableList(self, name : String) -> LoroMovableList`
- `pub fn LoroDoc::getText(self, name : String) -> LoroText`
- `pub fn LoroDoc::getTree(self, name : String) -> LoroTree`
- `pub fn LoroDoc::import(self, bytes : Bytes) -> Unit raise LoroError`
- `pub fn LoroDoc::export(self, opts : ExportOptions) -> Bytes`
  - 至少支持 `mode=update`，并允许 `from=VersionVector`（对齐 TS `doc.export({ mode:'update', from: version })`）
- `pub fn LoroDoc::oplogVersion(self) -> VersionVector`
- `pub fn LoroDoc::toJSON(self) -> Json`（或 `LoroValue` → JSON）

### 2.2 容器

- `Container`：所有容器的共同抽象（用于 `isContainer/getType`、容器引用展开、以及未来的订阅/事件统一入口）。
  - `pub fn Container::id(self) -> ContainerID`
  - `pub fn Container::kind(self) -> ContainerType`
  - `pub fn Container::doc(self) -> LoroDoc`（或内部弱引用句柄）
- `LoroMap`：键值对，值为 `LoroValue`（含容器引用）。
- `LoroList`：数组序列，元素为 `LoroValue`（含容器引用）。
- `LoroMovableList`：可移动列表（元素具备稳定 identity；`move` 通过 elem_id 语义而非“值移动”）。
- `LoroText`：RichText（字符串 + 样式 marks）。
- `LoroTree` / `LoroTreeNode`：可移动树（节点元数据为 Map 容器；与 Rust/TS 行为一致）。

---

## 3. 内部架构（建议分层）

```
moon/loro_codec/     # 已有：二进制编解码（Document/ChangeBlock/Op/Value…）
moon/loro_doc/       # 新增：运行时（oplog + state + CRDT 算法）
  core/              # ID/IdLp/VersionVector/Frontiers/DAG/Clock/Errors
  doc/               # LoroDoc, Txn, Container store, root registry
  containers/
    map/
    list/
    movable_list/
    text/
    tree/
  algo/
    seq/             # Text/List 共享：tracker/rope（Eg-walker 启发 + Fugue）
    tree/            # FractionalIndex + tree apply
  bridge/
    codec/           # Change/Op <-> runtime op 的转换、import/export 管道
```

关键原则：

- **codec 与 runtime 解耦**：codec 只负责字节 ↔ Change/Op IR；runtime 只负责 IR 的因果合并与状态演化。
- **正确性优先**：先做“能互通 + 状态一致”的版本，再做性能与 time-travel。

---

## 4. CRDT 算法落地策略（按容器拆分）

### 4.1 Map（LWW Map，先做）

- 状态：`key -> (value?, idlp)`；删除也是一次写入（value=None）。
- 合并规则：按 `(lamport, peer)`（或 IdLp）比较，取较大者。
- 本地操作：`set/delete` 直接产生 `MapOp`；commit 后进入 oplog。
- Rust 参考：
  - `crates/loro-internal/src/state/map_state.rs`
  - `crates/loro-internal/src/container/map/map_content.rs`

### 4.2 List（序列 CRDT：复用 Text 的 tracker/rope 思想）

Loro 的 List diff 计算复用 RichText 的 tracker（用 `Unknown` chunk 承载 list 插入的“长度”，真实值再回查 oplog）。

- 需要实现的核心组件：
  - `FugueSpan` / `Status`：表示一个插入 span（含 tombstone/future）、其 `origin_left/origin_right`。
  - `CrdtRope`：插入排序规则（并发插入的稳定排序）、删除（含 reverse 删除）、split。
  - `IdToCursor`：ID → rope cursor 的索引结构（支持 insert/delete 映射）。
- 落地建议：
  - v1：先做“仅维护当前版本”的 tracker（导入 updates 时在线更新）。
  - v2：补齐 `checkout(vv)` / `diff(from,to)`（支持 export-from、订阅 diff 等）。
- Rust 参考（必读）：
  - `crates/loro-internal/src/container/richtext/tracker/crdt_rope.rs`
  - `crates/loro-internal/src/container/richtext/tracker/id_to_cursor.rs`
  - `crates/loro-internal/src/container/richtext/tracker.rs`
  - `crates/loro-internal/src/diff_calc.rs`：`ListDiffCalculator`（Unknown span 回查 oplog 的处理）

### 4.3 MovableList（“位置序列 + 元素 identity”的组合 CRDT）

MovableList 不是简单的 List：

- 底层维护一条 **ListItem 序列**（其插入排序仍是序列 CRDT 的问题，和 List/Text 同源）。
- 每个用户可见元素有稳定 `elem_id`（`IdLp`），并维护：
  - `pos : IdLp`（最后一次有效 move/insert 的位置标识，对应某个 ListItem 的 idlp）
  - `value_id : IdLp`（最后一次有效 set 的写入标识，用于 LWW 取值）
- Move 的本质：在目标位置插入一个新的 ListItem（带新 op_id），然后把 `elem_id.pos` 更新到这个新 ListItem（按 LWW 决胜）；旧位置对应的 ListItem 变为“dead”（不再 `pointed_by`）。
- 用户视角 length 只统计 `pointed_by != None` 的 ListItem；op 视角 length 统计全部 ListItem（含 dead），因此需要 **双索引**（ForUser / ForOp）。

落地建议：

- v1：先实现“当前版本正确”的 apply（insert/delete/move/set + LWW 规则 + child container index）。
- v2：再接入通用 diff/checkout（若要实现 time-travel/订阅），沿用 Rust 的 MovableListDiffCalculator 思路。

Rust 参考（必读）：

- `crates/loro-internal/src/state/movable_list_state.rs`（核心状态结构与 move/set 语义）
- `crates/loro-internal/src/container/list/list_op.rs`（Move/Set/Insert/Delete 统一在 ListOp 中）
- `crates/loro-internal/src/diff_calc.rs`：`MovableListDiffCalculator`（与 ListDiffCalculator 的复用关系）

### 4.4 Text（RichText：TextChunk + Style anchors + styles range map）

Text 的本体是“文本插入/删除”，样式是“锚点事件（StyleStart/StyleEnd）”插入到同一条序列中：

- 需要实现：
  - `RichtextChunk`：Text / StyleAnchor / Unknown / MoveAnchor
  - tracker/rope：同 List（但 Text 需要正确处理 **Unicode 长度** 与 UTF-8 索引换算）
  - styles：将 StyleStart/End 解释为区间样式，生成 delta（或稳定 JSON 结构）
  - 本地 API：`insert/delete/mark/markEnd`（或提供更 TS 友好的 `mark(start,end,style)` 并在内部生成 start/end 锚点）
- Rust 参考（必读）：
  - `crates/loro-internal/src/container/richtext/fugue_span.rs`
  - `crates/loro-internal/src/container/richtext/richtext_state.rs`
  - `crates/loro-internal/src/state/richtext_state.rs`
  - `crates/loro-internal/src/diff_calc.rs`：`RichtextDiffCalculator`

### 4.5 Tree（Movable Tree：FractionalIndex + last-move-wins + cycle handling）

Tree 的关键点不是“序列插入”，而是“父子关系 + 同级顺序”的并发合并：

- 状态：
  - `node_id -> { parent, position(FractionalIndex?), last_move_op(IdFull) }`
  - `parent -> sorted children`（排序 key：`FractionalIndex + idlp`）
  - `deleted_root` 语义：删除即 move 到 `DELETED_TREE_ROOT`
- 本地操作：
  - `create(parent, index)` / `move(target, new_parent, index)`：需要生成 `FractionalIndex`（在 siblings 间生成；必要时触发“重排”）。
- Rust 参考（必读）：
  - `crates/loro-internal/src/state/tree_state.rs`
  - `crates/loro-internal/src/diff_calc/tree.rs`（checkout/diff 思路，后续实现 time-travel 时参考）
  - `crates/loro-internal/src/container/tree/tree_op.rs`
  - `crates/fractional_index/`（FractionalIndex 生成算法，建议直接 port 到 MoonBit）

---

## 5. 里程碑拆解（按“可验证闭环”推进）

### S0：规格整理（Spec Pack，必须）（1–3 天）

- 先完成 `moon/specs/09-lorodoc-spec-pack.md` 规定的 specs（10–17）与 `moon/LORODOC_SPEC_NOTES.md`。
- 明确对齐目标：以 Rust 行为为真值、以 TS API 为用户心智模型；把“并发决胜规则/边界条件/坐标系”写成可测试断言。

**验收**：所有关键规则都能在 specs 中定位，并能回链到对应 Rust 源码位置；实现阶段不再“边写边猜”。

### M0：接口与骨架（1–2 天）

- 建 `moon/loro_doc/` 模块骨架；定义 `LoroError`、`VersionVector`、`Clock`、`ContainerID`、`LoroValue` 的 runtime 侧视图（优先复用 `moon/loro_codec/*` 类型）。
- 落一个最小 `LoroDoc::new()` + `getMap/getList/getText/getTree`（先返回轻量 handle，内部先不实现逻辑）。

**验收**：Moon 项目可编译；API 形态固定。

### M1：Map MVP（正确性优先）（2–4 天）

- 实现 MapState（LWW）+ `doc.toJSON()`（仅 Map/List/Text/Tree 的递归 JSON 展开）。
- 实现 `import(update_bytes)`：decode → Change/Op → apply。
- 实现 `export(update, from?)`：从 oplog 选择 changes → encode。

**验收**：用 Rust 生成 updates，Moon import 后 `toJSON` 与 Rust `to_json/get_deep_value` 一致；Moon 的本地 set/delete 导出 updates，Rust import 后一致。

### M2：Oplog / Versioning 最小闭环（2–4 天）

- 维护 `VersionVector`（每 peer 最大 counter）、frontiers（如需要）与 lamport 生成规则。
- 本地 commit：把本地 ops 打包成 Change（deps=当前 frontiers / vv 投影）。
- 支持 `export(from=vv)` 的增量导出。

**验收**：对齐 TS 示例：`version=oplogVersion()`，编辑后 `export({from:version})` 能在对端增量合并。

### M3：Seq CRDT 基建（List / MovableList / Text 共享）（1–2 周）

- port `CrdtRope + IdToCursor + FugueSpan/Status`（先做 Unknown chunk 路径）。
- 产出一份 MoonBit 侧可复用的 `seq_crdt` 模块（被 List、MovableList、Text 复用）。

**验收**：并发插入排序与删除（含 reverse delete）行为与 Rust 一致（可用对照用例/小规模 fuzz）。

### M4：List + MovableList（1–2 周）

- **List**：实现 import/apply + API（insert/delete/push/toJSON）。
- **MovableList**：实现 import/apply + API（insert/delete/move/set/toJSON），并保证：
  - move 不复制值、只移动 identity
  - set 与 move 的并发按 LWW（`IdLp(lamport,peer)`）一致决胜

**验收**：List/MovableList 并发用例与 Rust 结果一致（含 move+delete+set 交织）。

### M5：RichText（Text + Styles）（1–2 周）

- 在 M3 的 rope 上补齐 TextChunk、unicode/utf8 索引换算、Style anchors。
- 实现 `mark`/`markEnd` 与 `toDelta`（或至少 `toJSON` 能包含样式信息）。

**验收**：对齐 Rust/TS 的 richtext 行为：文本内容一致、样式区间一致（至少在导出 json-schema/changes 的层面可对照）。

### M6：Tree（FractionalIndex + apply）（1–2 周）

- port `fractional_index` 到 MoonBit；实现 siblings 顺序与重排逻辑。
- 实现 TreeState：create/move/delete + children cache + meta container（Map）联动。

**验收**：对齐 Rust tree 典型用例与并发移动用例；无环约束与 deleted_root 语义一致。

### M7：订阅与事件（可选，后续）

- 提供 doc/container 级别 subscribe（回调 diff）。
- 若未来要对齐 wasm 的“microtask flush”语义，再单独设计（MoonBit runtime 不一定需要）。

---

## 6. “读 Rust → 提取文档”计划（把算法讲清楚再写 MoonBit）

建议在动手 port 前，先按 `moon/specs/09-lorodoc-spec-pack.md` 输出一组 specs；其中最核心的算法文档包括：

1. **seq-crdt（List/Text）**（`12-seq-crdt-spec`）：
   - `FugueSpan` 字段语义、Status 状态机、origin_left/origin_right 的计算规则
   - `CrdtRope::insert/delete` 的并发排序与 split 规则
   - `IdToCursor` 的索引结构与更新策略
   - “Unknown span 回查 oplog”的策略（List 特有）
2. **richtext-styles**（`14-richtext-spec`）：
   - StyleStart/End 如何占用 op_id 位置、如何与文本混排
   - unicode/utf8/event_index 三套坐标系换算
3. **tree-crdt**（`15-tree-spec`）：
   - last-move-wins 的比较键（lamport/peer）与 cycle 处理策略
   - FractionalIndex 的生成、碰撞与重排（generate_n_evenly）

写完文档后再开始 MoonBit 实现，可显著降低“盲 port”带来的返工风险。

---

## 7. 最终验收标准（面向 PR 合并）

- `LoroDoc::new()` + Map/List/MovableList/Text/Tree 的核心 API 可用，且行为与 `loro-crdt` TS 的直觉一致。
- Rust ↔ Moon 的 update bytes 互通：
  - Rust 生成 updates → Moon import → `toJSON` 与 Rust 一致
  - Moon 本地编辑 → export updates → Rust import → `get_deep_value/to_json` 一致
- 覆盖并发用例（至少：并发 list/text 插入、并发 movable_list move+set、并发 tree move、并发 map set 覆盖）。
