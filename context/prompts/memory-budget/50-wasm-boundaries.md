# 给 agent：WASM 短字符串直接绑定与大批量元数据预读

目标分支：主要从最新 `perf/phase4-global-wasm-memory-budget` 分叉；
元数据优化需要可拆分回 #1296 的提交。

## 仍有的性能回退

文本方法从 `&str` 改成 JS 引用，再 `as_string()` 回 JS 编码，短字符串也多一次边界往返。
原调查仅对 insert 的直接绑定原型有收益，尚未完成所有方法和最终包验证。
不要移除 ReportingAlloc：原调查中移除计数没有恢复主要文本回退。

设计 JS 侧短/长分流，短输入直接传参；长输入在复制进 WASM 前保留准入检查。
覆盖 insert、insertUtf8、splice、update、updateByLine，全部 nodejs/web/bundler/browser/base64
入口，以及消费者可能直接导入的 backing exports。不能只包装默认 index 然后留下绕过入口。
检查 UTF-16/UTF-8、ASCII/中文/代理对、malloc/realloc 峰值和小页数 V8 限制。
当前 3 bytes/code-unit 是最终 UTF-8 的保守上界，并非完整引擎临时峰值的证明。

小 importBatchDetailed 已改成每 blob 一次 WASM 拷贝，失败仍逐 blob 返回。
大批量仍先读元数据排序，再按需加载 payload，两次 WASM 拷贝尚未移除。
要通过按区间读取元数据等方式保留有界驻留、排序和完整校验；不要退回先复制所有大包。

## 验收

统一正常内容、事件投递（无 Event not called）、结构化额度错误、拒绝前线性内存不增长、
超额 tryCommit/import 回滚及随后小编辑、所有包入口。正常 JSON/binary 兼容不变。
记录 B4、短文本批量插入、ASCII/Unicode 大输入、单/大批次的 A/B 耗时与峰值。
必须 `CI=true pnpm release-wasm`；若改变打包/exports，跑 bundler smoke，浏览器矩阵先确认范围。
不要手改生成的包。所有对照需要可核对 source/head/binary，原调查原型只是线索。

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
