# 内存优化与额度控制：第二轮修复及交接

Verified against code 2026-11-16.

本轮承接评论中的剩余问题：完成已有明确修复方式的部分；需要额外设计或运行证据的部分，整理为可独立分发的 [6 份 prompts](../prompts/memory-budget/00-index.md)。这不表示整个 PR 栈已达到可合并状态。
上一轮记录见 [repair progress](memory-pr-repair-20261116.md)，其中 Pending 状态是当时的历史快照。

## 本轮修复

| 范围 | 修复与验证依据 |
| --- | --- |
| #1299 undo 未知容器 | 空 affected_cids 可能表示未知。保守组合远端变换；已知与未知合并后仍保持未知。正常同 peer 导入、checkpoint、另一 peer 前插的测试在修复前错误得到 `e-local`，修复后 undo 得到 `remote-`，redo 正确。 |
| #1299 有限 undo 字节额度 | 每次裁剪只统计一次总量，之后减去删除项及丢弃行的容量和远端 diff；测试验证统计次数与释放字节。无限额度继续不遍历。 |
| #1300 fork 额度继承 | Rust/WASM fork 和 fork_at 继承完整本地策略，修改互相独立。内部快照交接仍绕过外部 blob 上限，后续公开导入继续受限。 |
| #1300 shallow root 后端 | 导入 shallow snapshot 时，根状态使用预先配置的同类存储后端；落盘字节及 fork 内容测试通过。仍保留的 encoded_state_bytes 不在此修复范围。 |
| #1301 无需回收时的缓存 | 先判断目标，再清共享块缓存；低于目标时保留热缓存。 |
| #1301 重复 Stage 1 | 记录本轮成功完成的文档，Stage 2 不重复清缓存和压缩；首轮忙碌的文档仍会重试。内部测试验证两轮只执行一次 Stage 1。 |
| #1301 容器句柄读取的 LRU | 通过已保留句柄读取字段、浅值、深值时更新访问记录；测试证明较冷文档先降级，热文档内容不变。 |
| #1301 小批量 detailed 导入复制 | 小批量与单包共享元数据和导入的 Bytes，一包只复制进 WASM 一次；保留单包边界、每包额度错误与混合结果语义。Rust owned 路径使用迭代器，不额外收集第二个 Vec。 |

用户已确认的规则保持不变：`set_memory_budget` 的新额度从下一笔事务生效，保留已有未提交编辑。

## 验证

- `cargo check -p loro-internal`、`cargo check -p loro-wasm --target wasm32-unknown-unknown`：通过。
- `cargo test -p loro-internal --lib`：550 通过，4 ignored。
- public 聚合检查：`apply_diff_atomicity` 16、`global_memory_budget` 8、`import_batch_bytes` 4、`import_batch_detailed` 12、`memory_budget` 13、`out_of_core` 2、`undo_memory_limits` 1；合计 56 通过。
- 另跑 undo 模块 12、undo_split_spans 5、pending_snapshot 3、fork_at_deleted_container 14，以及 shallow-root 后端回归，均通过。
- `pnpm test-loom`：10 通过。
- `CI=true pnpm release-wasm` 完成所有包构建；首次测试暴露新增断言错误，随后修正断言和测试自身的 `this` 类型。最终 `CI=true pnpm -C crates/loro-wasm test`：声明、CommonJS、TypeScript 通过，Vitest 583 通过（2 skipped、2 todo），Deno 4、Bun 4 通过。生产代码和构建产物在两次包装测试之间未变。
- `cargo fmt --all -- --check`、`git diff --check`：通过。

本轮没有重测端到端耗时或峰值内存，不作新的性能百分比承诺。复制次数、统计遍历次数与回收轮数由定向回归验证。上一轮基准的旧 baseline 复用和无预算约 +6.5% 的限制继续成立，需独立重建 AB 对照。

## 留给其他 agent 的任务

1. [导入回滚自身失败](../prompts/memory-budget/10-import-rollback.md)：需要状态 journal 或预先验证的恢复计划，不能吞错或无条件先回滚历史。
2. [全局生命周期与记账](../prompts/memory-budget/20-global-lifecycle-accounting.md)：最后句柄/注册表竞争尚未复现；原生准确统计、回收退避及物理页额度语义需要设计。
3. [落盘存储](../prompts/memory-budget/30-spill-storage.md)：增量 flush、真实 IO 失败、目录重试、保留输入的所有权问题。
4. [内存统计](../prompts/memory-budget/40-memory-accounting.md)：盒装 Op payload 的增量统计、StrArena 实际释放与峰值。不能以每次提交全量遍历修复漏记。
5. [WASM 边界](../prompts/memory-budget/50-wasm-boundaries.md)：短字符串直接绑定、大批次元数据读取及临时分配峰值。
6. [JSON/checkout/性能对照](../prompts/memory-budget/60-json-checkout-and-control.md)：尚未证明混合冷热访问不回退的优化，以及新的完整 AB 基线。

每份 prompt 区分代码观察、已复现故障与待确认疑点，列出已修边界和验收要求。未自动通知或启动其他 agent。

## 分支交付

#1296 保持 `d2e88876`；#1299 新提交 `a6fdea5a` 已推送；#1300 在新 phase2 上重放后，新提交 `22c25552` 已推送。#1301 在新 phase3 上重放，本报告与本轮全局/WASM修复一同提交。最终头提交以交付时远端核验结果为准。没有合并或部署。
