# 给 agent：落盘 flush、后端切换失败和仍被保留的输入缓冲区

目标分支：从最新 `perf/phase3-compact-ops-and-out-of-core` 分叉。

## 证据与待解决点

- `FileKvStore::flush` 经 `MemKvStore::flush/export_all` 重写整表。
  不是只追加本次 memtable；反复压力回收会带来与全部历史等大的 IO 和内存峰值。
- `set_memory_budget` 先保存新 `spill_dir`，`make_backends` 失败只 warn；
  再次设置同一路径可能因“没有变化”而跳过重试。
  仅增加一个返回 `create_dir_all` 错误的 try API 不够：后续 spill 写入失败
  目前也可能只记录日志，需要真实区分切换成功、仍留在 RAM 和 IO 失败。
- `GcStore::encoded_state_bytes` / `pruned_root` 和缓存块的 encoded-data views
  仍可能持有大输入。小 key 留住输入已修，不能因此宣布任意热缓存路径都释放了输入。
- 只有 `spill_dir`、没有额度/水位线时，没有明确的自动回收触发阈值。
  先界定目录只是后端选择，还是承诺自动冬眠；不要擅自加一个会让主路径不可用的额度。

## 已修边界

Unix 临时文件 0600；克隆共享不可变 SpillFile；fork 复制完整独立预算；
预先配置 spill 后的浅快照根状态使用相同后端。
浅根后端已落盘不等于 encoded root 副本已经释放。

## 需要方案和测试

为 flush 设计合理的增量/分层合并策略。直接把新 memtable 做成 should_encode_none=false
的小表会丢墓碑，旧表里的删除项可能复活；永不合并又会无限增长文件/元数据。
要覆盖覆盖写、删除、重复 flush、scan/双向 scan、export、clone/fork、切换和最后持有者清理。
用 IO 计数/峰值分配证明“少量编辑不每次重写大历史”。

为目录切换提供向后兼容的失败反馈，失败不记录未实现的目录，修正条件后同目录能重试；
含目录路径实际是文件、受控写失败、partial backend migration，内容不能丢。
用 owner-drop/counter 测试确认冷/热正常输入在 spill、重写、回收、drop 后的实际释放；
不能只看 estimated_memory_bytes 或 RSS。
起步：KV lib 测试、`cargo test -p loro --test out_of_core --test memory_budget --test pending_snapshot`。

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
