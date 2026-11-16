# 给 agent：导入超额拒绝时，恢复操作本身失败的原子性

目标分支：从最新 `perf/phase2-doc-memory-budget` 分叉；如需验证全局额度，
再在独立集成分支叠到最新 phase4。

## 问题与证据

入口在 `crates/loro-internal/src/loro.rs`：
`import_changes_and_apply_delta_to_state_if_needed`、`revert_state_silently`、
`BatchImportGuard::finish`、快照初始化的回滚分支。
单包额度拒绝的状态恢复调用使用 `?`，恢复失败会跳过 created journal 和
OpLog scope 清理；批量分支则可能在状态恢复失败后仍回滚历史，使状态领先历史。
本轮只重新读了这些分支，尚未构造恢复失败的完整运行证据。
`frontiers_to_vv_or_not_found` 已替代旧的 `unwrap_or_default`，不要重复处理。

## 需要完成

1. 用正常生成的文档与受控 failpoint 确认逆向 diff 计算或应用失败时的状态。
2. 设计在修改状态前可验证的回滚计划，或触及容器的状态 journal。
   不要简单把 `?` 改成吞错、panic 或无条件回滚 OpLog；也不要默认克隆整份文档。
3. 单包、批量、detailed、初始 full/shallow snapshot、detached、pending
   都要在失败后保持合理的一致状态；未提交编辑和原本的事件不能丢。
4. 验证恢复不需要重新读取可能已经被拒绝/截断的历史。

## 验收

拒绝前后比较 deep value、state/oplog frontiers、版本、pending、创建容器、
事件、scope 关闭状态。失败后能继续小编辑、导出、导入、checkout。
测量额外回滚计划的峰值内存和无压力导入耗时，不能用整份副本换正确性后
声称保留了内存收益。
起步：`cargo test -p loro --test memory_budget --test import_batch_detailed --test pending_snapshot --test apply_diff_atomicity`；
内部 `import_atomicity`，涉及 WASM 时重建并验证包装层。

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
