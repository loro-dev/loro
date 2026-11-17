# 给 agent：紧凑 Op 的间接分配记账与 StrArena 回收

目标分支：从最新 `perf/phase3-compact-ops-and-out-of-core` 分叉。

## 明确的缺口与不能直接采用的改法

`InnerContent::Map(Arc<MapSet>)`、`InnerListOp::Set(Arc<SetListOp>)`、
`StyleStart(Arc<StyleOp>)` 把 payload 移到了堆上。
`ChangeStore::estimated_memory_bytes` 和 `Transaction::estimated_uncommitted_bytes`
主要按 `size_of::<Op>() * len` 记账，遗漏这些间接分配。

不要直接在每次 commit 的 stats 中遍历所有历史 Op 来补总量：刚修好的页统计
把 10,000 Map / 2,000 commit 的预算成本从约 76ms 降到约 2.5ms（旧基线为复用产物）。
全量逐 Op 扫描可能把回退重新带回来。需要按不可变块/变更缓存并在追加、解析、
回滚、降级、compaction 等入口保持准确，或提出其它有复杂度证据的方案。
保留 64-bit `size_of::<Op>() <= 40`，不要把所有 shared Arc payload 无解释地重复计入。
先明确“保守每文档估计”和唯一实际 heap 之间的边界，包含嵌套值与 Arc header。

`StrArena::shrink_spare_capacity` 目前复制整个 AppendOnlyBytes 到 exact-fit 缓冲区。
旧 BytesSlice 尚在时可能没有释放旧分配；下一次 append 又会增长复制。
需要验证 shared views、checkpoint/rollback、长会话和峰值，不能只看新 capacity 变小。
不确信能证明收益时保留并明确报告，不用激进复制冒充回收。

## 验收

以分配计数/owner 生命周期和工作量计数约束新增统计，不只比较自我计算结果。
覆盖 txn、parsed-only/Both/bytes-only blocks、shared spans、append、rollback、重解析、
fork、64-bit/WASM。运行预算/回滚/落盘窄测试、内部 lib、Loom。
重新构建可核对 head 的 A/B 基线，检查无预算、单文档预算、真实堆全局预算的热提交。

## 工作约束与当前状态

仓库为 `loro-dev/loro`。先读根和目标目录的 `AGENTS.md`、
`context/memory-budget.md`、`context/reports/memory-followup-20261116.md`。
先 `git status --short --branch`，再核对最新远端分支。旧调查 head
`3e3eb6fb / 1b8c4dc4 / 35510eff / 4cefe825` 已过期，不能覆盖当前修复。
在隔离分支/工作树完成此项，返回可 cherry-pick 的提交；不要直接改四个
stacked PR 的共享分支、合并或部署。若环境不允许 Git 写入，报告限制。

必须保留：硬额度、失败原子回滚、冷状态降级、流式导出，以及已确认的
“新额度从下一笔事务生效、保留已有未提交编辑”规则。
先复现/核实，再修复；尚未证实的疑点不能写成已确认故障。
增加正常文档和受控故障注入回归，先跑窄测试；长 fuzz/browser 矩阵先询问。
发布行为变更加 changeset，更新过期 context。不要手改生成的 WASM 包。
最终列出逐项判定、实际测试命令/结果、性能和验证限制、提交 SHA。
