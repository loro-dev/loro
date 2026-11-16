# 给 agent：全局文档注册表的生命周期、原生统计成本和回收语义

目标分支：从最新 `perf/phase4-global-wasm-memory-budget` 分叉。

## 仍待处理

1. `snapshot_live_docs` 把 Weak 升级成 `Arc<LoroDocInner>`，但
   `lib.rs::Drop for LoroDoc` 用 `Arc::strong_count == 1` 判断最后一个句柄。
   临时注册表引用可能改变最后句柄的提交时机。这一条件已读码确认，
   丢操作的线程窗口尚未复现。`BasicHandler` 也持有 `LoroDoc`，
   `from_inner` 在内部回调/事务中广泛使用，不能草率加一个“用户句柄数”就结束。
2. 没有 allocator tracker 的原生路径，每次额度检查仍可能快照/重算所有文档。
   不能直接用陈旧 cached total 取代：纯读取会加载状态和缓存，忙文档不能漏记。
   研究准确失效标记/增量统计，保留无预算 Loom 快速路径。
3. 无法回收到目标时，连续提交可能反复做完整软回收。设计有进展判断的退避，
   硬额度检查不能跟着跳过；不要让新活跃状态因为永久退避而不再回收。
4. `try_reclaim_global_to_budget` 检查实际线性页大小，`tryCommit` 则可在页不增长且
   live heap 已下降时继续写。先判断这是合理的“物理超额诊断 vs 写入准入”区别，
   还是需要改变 API；WASM 页不会缩小，不能假装回收后页数会下降。

## 已修，不要撤回

低 live heap 时跳过全局软统计；高页数需要同时有 live heap 压力；
Stage 2 不重跑已完成的 Stage 1；低于 target 不清共享块；保留句柄读取刷新 LRU；
预算 setter 不在已有编辑/嵌套 batch 中插入新的回滚 scope。

## 验收

用屏障/受控钩子确定性覆盖“最后实际句柄 drop 与注册表扫描交错”，
验证 local-update/提交事件、历史、数据以及析构时锁顺序。
1/100/1000 个文档的单文档提交需有复杂度证据，含读取后再提交、忙文档、
drop、global policy 切换。不要只做单线程耗时或空文档计数。
运行 `cargo test -p loro --test global_memory_budget --test multi_thread_test`、
`pnpm test-loom`，必要时增加有预算的专用 Loom 模型；WASM 语义变化需包测试。

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
