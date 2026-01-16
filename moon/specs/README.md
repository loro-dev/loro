# Moonbit 实现 Loro 编码格式：规格与计划索引

本目录用于记录“用 Moonbit 实现 `docs/encoding.md` 所描述的 Loro 二进制编码格式”的实现计划、关键规格摘记与测试/验收策略。

## 文档列表

- `moon/specs/00-goals-and-acceptance.md`：目标、范围、验收标准（以跨语言 e2e 互通为准）
- `moon/specs/01-context-checklist.md`：开工前必须收集/确认的 Context 清单（规格/源码/边界条件）
- `moon/specs/02-module-plan.md`：按模块逐步实现的详细计划（每步的依赖、测试与退出条件）
- `moon/specs/03-e2e-test-plan.md`：e2e 测试方案（Rust 向量生成、Moon CLI 约定、对照校验）
- `moon/specs/04-ir-design.md`：Moon 侧 Change / Op 数据结构设计（重点：Change / Op 结构与测试友好 JSON 形态）
- `moon/specs/05-fastupdates-changeblock-encoding.md`：FastUpdates / ChangeBlock 的编码细节摘记（以 Rust 源码为真值），用于实现 mode=4 的真正 decode→encode
- `moon/specs/06-jsonschema-export.md`：JsonSchema（`docs/JsonSchema.md`）导出实现细节（FastUpdates 二进制 → JsonSchema JSON）
- `moon/specs/07-jsonschema-encode.md`：JsonSchema 编码实现细节（JsonSchema JSON → FastUpdates 二进制）
- `moon/specs/08-lorodoc-basic-plan.md`：MoonBit 侧 LoroDoc 运行时（CRDT 引擎）基础支持计划（Container/Map/List/MovableList/Text/Tree）
- `moon/specs/09-lorodoc-spec-pack.md`：LoroDoc Runtime 的 Spec-driven 开工包（步骤 0：阅读/整理 specs + Rust 真值索引）
- `moon/specs/10-lorodoc-api-spec.md`：LoroDoc runtime 对外 API 规格（对齐 TS 心智模型）
- `moon/specs/11-lorodoc-core-model-spec.md`：Core model（ID/Lamport/Change/DAG/vv/frontiers）
- `moon/specs/12-seq-crdt-spec.md`：List/Text/MovableList 共享的序列 CRDT（Fugue + Eg-walker 启发）
- `moon/specs/13-movable-list-spec.md`：MovableList（identity + move/set 的并发语义）
- `moon/specs/14-richtext-spec.md`：RichText（样式 anchors + expand + toDelta）
- `moon/specs/15-tree-spec.md`：Tree（FractionalIndex + last-move-wins + cycle/rearrange）
- `moon/specs/16-lorodoc-import-export-spec.md`：Import/Export updates（mode=4）语义与 codec 对接
- `moon/specs/17-lorodoc-test-plan.md`：runtime 测试与验收计划（Rust 作为 oracle）

## 约定

- Moonbit 代码统一放在 `moon/` 下（例如 `moon/loro_codec/`、`moon/bin/`、`moon/tests/`）。
- “正确性”的最终定义：Rust ↔ Moon 的导出/导入能互相 decode/encode，并在 Rust 侧用 `get_deep_value()`（或等价接口）验证状态一致。
