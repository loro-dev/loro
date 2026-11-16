# 给 agent：JSON 反复解码、checkout 重复块查询和未开启预算的对照回退

目标分支：从最新 `perf/phase1-speed-and-peak-memory` 分叉，必要时在最新 phase4 上测集成效果。

## 需要研究并决定是否采用

- ephemeral bulk reads 避免大文档把全部解码值留在 RAM，但小文本反复 toJSON 也重复解码。
  原调查的 1 MiB 字节缓存候选改善了小文本，尚未证明冷读/大混合文档/抖动不退步。
  不要简单把所有 ephemeral reads 改回持久缓存。覆盖根容器增删、隐藏/复活、缓存零额度、
  标记更新、超大单容器、反复读和超过工作集的循环读取。
- 重放按容器反复探测 CID header，再 get_change 查询同一历史块。
  优先研究把探测和获取合并，保留冷块跳过收益；不能凭未经验证的 header 跳过损坏块体。
  不取消关键版本认证，不把 meet 当作安全重放点。改 diff mode/重放基准前读 critical-version-spec。
- 本轮之前的原生窄测试：预算提交 76.398 -> 2.530 ms，但无预算对照 1.299 -> 1.383 ms
  （约 +6.5%）。基线复用旧调查 rlib，不是本轮重建；结果在
  `benchmarks/memory-budget-repair-20261116/`。不要说所有无预算场景已经无回退。
- #1296 普通 KV 块为恢复墓碑剔除/小块合并而经过 add_new_block，large block 仍 raw copy。
  对正常块增量导出的 CPU/峰值尚未重测；优化不能丢墓碑或永久制造零碎小块。

## 验收

重新构建最新各层和 main 的可核对基线，交错 A/B、多轮区间、相同 fixture/工具链，
分别报告 B4、JSON 冷/热、大混合文档、checkout、导出、无预算/有预算。
保留 raw、二进制哈希和内容 oracle；时间与内存分开，不能跨轮次相减。
先窄测 owned batch、fork、正常历史读取和重放；影响 WASM 时全包验证。
重复代码/allow(dead_code) 清理不是已确认回归，不要混入高风险语义修复来凑完成项。

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
