# 剩余内存预算问题：可分发 prompts

Verified against code 2026-11-16.

这些是本轮明确修复完成后仍需要设计或额外证据的任务。每个文件可独立交给一个 agent，
都包含工作范围、证据强弱、已修边界和验收要求。没有自动创建或通知任何 agent。

| Prompt | 优先级 | 内容 |
| --- | --- | --- |
| [10](10-import-rollback.md) | 最高 | 导入被拒绝后，状态恢复自身失败的原子性 |
| [20](20-global-lifecycle-accounting.md) | 高 | 注册表/最后句柄生命周期、原生全局统计、回收退避与 API 语义 |
| [30](30-spill-storage.md) | 高 | 增量 flush、真实 IO 失败反馈、目录重试与仍被保留的输入 |
| [40](40-memory-accounting.md) | 高 | 盒装 Op 的增量记账、StrArena 真实回收 |
| [50](50-wasm-boundaries.md) | 性能 | 所有入口的短字符串绑定、大批次元数据读取 |
| [60](60-json-checkout-and-control.md) | 性能 | JSON 热缓存、checkout 查询合并、无预算/导出性能对照 |

共享代码重叠：10/20/30/40/50 都可能修改 `loro.rs`；请各用隔离分支，返回小提交后统一集成。
不要让多个 agent 同时写原始工作树或覆盖四个 PR 分支。
本轮完成结果见 [follow-up](../../reports/memory-followup-20261116.md)。
