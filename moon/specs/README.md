# Moonbit 实现 Loro 编码格式：规格与计划索引

本目录用于记录“用 Moonbit 实现 `docs/encoding.md` 所描述的 Loro 二进制编码格式”的实现计划、关键规格摘记与测试/验收策略。

## 文档列表

- `moon/specs/00-goals-and-acceptance.md`：目标、范围、验收标准（以跨语言 e2e 互通为准）
- `moon/specs/01-context-checklist.md`：开工前必须收集/确认的 Context 清单（规格/源码/边界条件）
- `moon/specs/02-module-plan.md`：按模块逐步实现的详细计划（每步的依赖、测试与退出条件）
- `moon/specs/03-e2e-test-plan.md`：e2e 测试方案（Rust 向量生成、Moon CLI 约定、对照校验）
- `moon/specs/04-ir-design.md`：Moon 侧 Change / Op 数据结构设计（重点：Change / Op 结构与测试友好 JSON 形态）
- `moon/specs/05-fastupdates-changeblock-encoding.md`：FastUpdates / ChangeBlock 的编码细节摘记（以 Rust 源码为真值），用于实现 mode=4 的真正 decode→encode

## 约定

- Moonbit 代码统一放在 `moon/` 下（例如 `moon/loro_codec/`、`moon/bin/`、`moon/tests/`）。
- “正确性”的最终定义：Rust ↔ Moon 的导出/导入能互相 decode/encode，并在 Rust 侧用 `get_deep_value()`（或等价接口）验证状态一致。
