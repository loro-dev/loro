# 09. LoroDoc Runtime – Spec Pack（Spec-driven 开工包）

本文件定义 MoonBit 侧实现 LoroDoc（运行时 CRDT 引擎）时，**开工前必须完成的规格整理（步骤 0）**与“应当产出的 specs 列表”。

目标：在开始写 `moon/loro_doc/` 代码之前，把“行为规则”从 Rust/TS/文档中抽取成可执行的、可对照的内部规格，避免边写边猜导致返工。

> 约定：Rust 实现是最终真值（source of truth）。Spec 的作用是把真值“压缩成可实现的规则 + 测试断言 + 关键边界条件”。

---

## 9.1 步骤 0：规格整理（必须）

### 9.1.1 阅读/对照材料清单

优先级从高到低：

1. Rust 源码（真值）
2. `docs/`（仓库内文档，主要是 encoding；运行时语义需要从源码归纳）
3. `loro-crdt`（TS）对外 API 与行为（作为“用户心智模型”与命名参考）
4. loro.dev 的教程/博客（如需要补语义背景；以 Rust 行为为准）

### 9.1.2 必须产出的 specs（本目录内）

> 这些文件是“写代码前的 gate”。未完成则不进入实现阶段（`08` 里的 M0+）。

1. `moon/specs/10-lorodoc-api-spec.md`
   - LoroDoc/Container/Map/List/MovableList/Text/Tree 的对外 API（方法名、参数、返回、错误）
   - `toJSON`/容器引用（`LoroValue::Container`）的展开规则
2. `moon/specs/11-lorodoc-core-model-spec.md`
   - `ID/IdLp/ContainerID/VersionVector/Frontiers/Lamport` 的定义与比较规则
   - Change/Op 的最小运行时语义：deps/frontiers、commit、op_id 分配、导出增量选择
3. `moon/specs/12-seq-crdt-spec.md`（List/Text 共享）
   - `FugueSpan/Status/origin_left/origin_right` 语义
   - `CrdtRope` 的并发插入排序与删除（含 reverse delete）规则
   - `IdToCursor` 的索引结构与 split 更新规则
4. `moon/specs/13-movable-list-spec.md`
   - MovableList 的两层结构：ListItem 序列 + Element（elem_id/pos/value_id/value）
   - `move/set/insert/delete` 的并发决胜（LWW，按 `IdLp(lamport,peer)`）
   - ForUser/ForOp 两套 index 的定义与转换
5. `moon/specs/14-richtext-spec.md`
   - 文本 chunk（unicode/utf8/event_index 坐标系）与 style anchors 语义
   - `mark/markEnd` 的 op 形态、样式区间解析、`toDelta`（或等价输出）
6. `moon/specs/15-tree-spec.md`
   - TreeState 合并规则（last-move-wins）、cycle 处理、deleted_root 语义
   - FractionalIndex 生成、碰撞与重排（`generate_n_evenly` 等）
7. `moon/specs/16-lorodoc-import-export-spec.md`
   - update bytes 的 import/export 语义（`from: VersionVector` 增量）
   - “选择哪些 changes 导出”与“如何保持可合并”的规则
8. `moon/specs/17-lorodoc-test-plan.md`
   - Rust ↔ Moon 的向量格式（bytes + 真值 JSON + 可选变化日志）
   - 并发用例最小集合（map/list/movable_list/text/tree）
   - 小规模随机/差分测试策略（以 Rust 为 oracle）

### 9.1.3 必须输出的 notes（实现查阅用）

- `moon/LORODOC_SPEC_NOTES.md`
  - 把上述 specs 的关键“判定规则/边界条件/坑位”浓缩记录下来（类似 `moon/SPEC_NOTES.md` 之于 codec）。

---

## 9.2 Rust “真值”索引（运行时语义）

> specs 编写时需要把每条关键规则都挂到明确的 Rust 文件/模块上，便于追溯与后续升级。

### 9.2.1 文档/容器/状态主入口

- `crates/loro-internal/src/loro.rs`
- `crates/loro-internal/src/state/*.rs`（各容器 state）
- `crates/loro-internal/src/oplog/*.rs`（oplog、change store、dag）

### 9.2.2 Map（LWW）

- `crates/loro-internal/src/state/map_state.rs`
- `crates/loro-internal/src/container/map/map_content.rs`

### 9.2.3 List / Text（seq crdt 基础）

- `crates/loro-internal/src/container/richtext/tracker/crdt_rope.rs`
- `crates/loro-internal/src/container/richtext/tracker/id_to_cursor.rs`
- `crates/loro-internal/src/container/richtext/fugue_span.rs`
- `crates/loro-internal/src/container/richtext/richtext_state.rs`（TextChunk/坐标系/样式结构）
- `crates/loro-internal/src/diff_calc.rs`（ListDiffCalculator/RichtextDiffCalculator 的使用方式）

### 9.2.4 MovableList

- `crates/loro-internal/src/state/movable_list_state.rs`（elem_id/pos/value_id、ForUser/ForOp）
- `crates/loro-internal/src/diff_calc.rs`（MovableListDiffCalculator 与 List 的复用关系）

### 9.2.5 Tree

- `crates/loro-internal/src/state/tree_state.rs`
- `crates/loro-internal/src/diff_calc/tree.rs`（checkout/diff 的思路，后续做 time-travel/订阅时参考）
- `crates/fractional_index/`

---

## 9.3 开工 gate（建议）

- 完成 `9.1.2` 的 8 份 specs（至少写到“可按步骤实现 + 可写测试断言”的粒度）
- `moon/LORODOC_SPEC_NOTES.md` 建好索引（每条关键规则都可回链到 Rust）
- 在 `moon/specs/08-lorodoc-basic-plan.md` 的 M0 之前，把“要实现什么”锁死，避免 API 漂移

