# 给 agent：内存预算剩余问题（接 followup-fixes PR 继续）

Verified against code 2026-11-16（基于 `perf/memory-budget-followup-fixes`）。

本文件自包含，取代 [10–60](00-index.md) 作为当前待办清单。10–60 是历史分组：
其中一部分已在下文“已完成”中修复，不能原样当作全部待修。

## 起点与交付方式（必读）

- 仓库 `loro-dev/loro`。先 `git status --short --branch`，读根 `AGENTS.md`、
  `crates/loro-internal/AGENTS.md`（涉及 WASM 再读 `crates/loro-wasm/AGENTS.md`）、
  `context/memory-budget.md`。
- 基线分支：`perf/memory-budget-followup-fixes`（draft PR，base 为
  `perf/phase4-global-wasm-memory-budget`，即 #1301）。获取链接与最新 head：
  `gh pr view perf/memory-budget-followup-fixes --repo loro-dev/loro --json url,number,headRefOid,baseRefName`
  并 `git fetch origin perf/memory-budget-followup-fixes`。
- 若已有以它为 base 的后续栈，接在最新 tip 上：
  `gh pr list --repo loro-dev/loro --search "base:perf/memory-budget-followup-fixes" --json number,headRefName,url`
  （再对查到的分支重复，直到没有后继）。
- 从该 tip 在隔离工作树创建新分支，完成后推送并创建 **draft** PR，`--base` 填该 tip 的分支名。
  **禁止**以 `main` 为 base，禁止推送/覆盖 #1296/#1299/#1300/#1301 四个分支或
  `perf/memory-budget-followup-fixes` 本身，禁止合并或部署。
- 不要重复或撤回下文“已完成”的修复，也不要撤回更早的 8 组修复（见
  `context/reports/memory-followup-20261116.md`）。
- 保留：硬额度、失败原子回滚、冷状态降级、流式导出；用户已确认“新额度从下一笔事务生效、
  保留已有未提交编辑”。不默认整文档克隆、每次全历史扫描、吞错或放宽硬额度。
- 共享机器：编译/测试用 `timeout <秒> flock /root/e2ee-review/test.lock <cmd>`，
  `CARGO_PROFILE_DEV_DEBUG=0`；长 fuzz、浏览器矩阵先询问。
- 发布行为变化加 changeset；改代码同时更新 `context/memory-budget.md`；不手改生成的 WASM 包。

## 已完成（followup-fixes PR，勿重复）

1. 最后句柄 drop 与注册表临时 `Arc` 竞争丢未提交操作：`DocDropGate`（`lib.rs`）+
   `RegistryRef`（`loro.rs`）。已复现（并发 `global_memory_stats` 时 2 万次 drop 丢 1 万余次），
   修复后 0；含 Loom 模型 `crates/loro/tests/global_registry_loom.rs`（对旧规则会失败）。
2. 盒装 Op 载荷记账：`InnerContent::payload_heap_bytes`、`Transaction::local_ops_payload_bytes`、
   `ChangesBlock::parsed_bytes`（变更失效缓存）。已复现 20×512 KiB 覆盖写在 2 MiB 额度下被
   `try_commit` 接受，修复后拒绝。
3. 拒绝导入的原子性回归测试（更新、非空文档导入快照、批量）。**回滚自身失败未修**，见 A。
4. 落盘目录切换失败：不再记录失败目录、同目录可重试；新增 Rust `try_set_memory_budget`；
   两个新后端先填充并 flush，均无写错误才安装。
5. 全局软回收退避（`soft_reclaim_backed_off`），硬额度检查不受影响；原生估算每次少扫一遍注册表。

## 待办（按建议顺序）

每项标注：**复现**＝有运行证据；**代码确定**＝读码确定、未运行复现；**猜测**＝待证实。

### A. 导入回滚自身失败时的原子性（代码确定；需要用户先定语义）

- 位置：`loro.rs` 中 `update_oplog_and_apply_delta_to_state_if_needed` 与
  `import_changes_and_apply_delta_to_state_if_needed` 的 `revert_state_silently(...)?`，
  以及 `BatchImportGuard::finish` 中 `revert_res` 失败后仍 `rollback_import`。
- 后果：单包路径返回回滚错误，状态与历史停在新版本但导入回滚 scope 和 created journal 不关闭
  （之后导入不再拥有 scope、历史压缩一直跳过、新容器一直被钉住）；批量路径状态领先历史。
- 触发条件未复现：逆向 diff 读取历史失败（例如懒加载块体损坏）。落盘读失败目前 panic 而非 Err（见 E）。
- 已试并放弃的方案：在改状态前预先算逆向 diff（“先算后改”）。在 20,000 op 的文本更新导入上，
  有硬额度时导入中位数 27.5 → 78 ms，额外峰值 15.5 → 36 MB（同 fixture、同工具链、A/B 交错两轮），
  对内存额度功能代价过大，未提交。
- 约束：导入返回 Err 时必须完全回滚；“保留本次导入但报错”或只关 scope 不算修复。
- 可研究：只在可能超额时预算（需证明估计为上界）、状态侧按容器的变更 journal、或回滚失败时
  从回滚后的历史重建受影响容器（仅读旧历史）。先用 failpoint 构造“逆向 diff 计算失败”复现。
- 验收：失败前后比较 deep value、oplog/state frontiers、pending、事件、scope 状态、created 容器；
  失败后可编辑/导出/导入/checkout；报告无压力导入耗时和峰值的 A/B。

### B. 原生（无 allocator tracker）全局统计随文档数线性增长（复现）

- 1 TiB 额度、无压力，1/100/1000 个文档的单文档 `try_commit`：2.4 / 60.5 / 981 µs；
  `commit`：2.0 / 26.4 / 314 µs（本 PR 之前测得；本 PR 只去掉了一次重复扫描）。
- 原因：`check_global_budget_with_active`、`enforce_global_watermarks_for_active` 每次
  `snapshot_live_docs` + 对每个文档 `try_stats_for_global`。
- 思路：文档在自身持锁操作结束时更新 `cached_doc_bytes`；纯读取导致状态加载时标脏；全局检查
  只重算脏文档 + 当前文档；debug 断言与全量重算一致。风险是漏失效导致少算；忙文档不能漏记。
- 验收：1/100/1000 文档复杂度证据（含读后提交、忙文档、drop、策略切换），`pnpm test-loom`。

### C. WASM 物理页额度语义（代码确定；需要用户决定，未经决定不要改）

- 线性内存页超过 `max_wasm_linear_bytes` 后，`try_reclaim_global_to_budget` 永远报错（页不缩小），
  而 `tryCommit` 在页未增长且 live heap ≤ 70% 时允许写入。用户已要求本轮不改物理页额度语义。

### D. KV flush 整表重写与每文档 Stage 1 无退避（复现）

- 2000 KB 已落盘历史，每次“小编辑 + `reclaim_memory`”写盘约 2000 KB（5/5 次）。
  `MemKvStore::flush → export_all → export_with_encoded_block` 解压并重建所有普通块，整表先在内存中
  构建（峰值约等于压缩后的全部历史）再写一个新文件。
- 每文档 `enforce_watermarks_with_budget_locked` 的 Stage 1 在 `spill_dir` 设置时每次超过高水位都
  `flush_kv`，没有类似全局路径的退避。
- 思路：分层 LSM——flush 只写新的小表并保留墓碑；仅合并到最底层时丢墓碑；按大小分层合并，合并时
  边读边写文件；`export_all` 流式合并。注意状态 KV 用 `should_encode_none(false)`，需要按表区分
  是否底层，否则删除项可能复活。
- 验收：覆盖写、删除、重复 flush、正反向 scan、export、clone/fork、后端切换、最后持有者清理；
  IO 计数/峰值分配证明少量编辑不重写全部历史。

### E. 落盘 IO 错误（代码确定；读错误语义需要用户决定）

- 切换后的写失败仍只记日志，表留在 RAM（不丢数据）。`KvStore::take_spill_error` 已存在但没有公开 API；
  是否以及如何暴露（统计字段、`take_spill_error` 方法、事件）需要决定。
- 读失败：`kv-store/src/sstable.rs` 的 `read_block`/`with_raw_block`/`export_all` 用
  `expect("spilled SSTable ... readable")` panic。改为 Err 需要改 `KvStore` 读接口；是否保持 fail-fast 需用户决定。
- 从落盘文件读回的块是否重新校验 checksum：**猜测**未校验，未核实。

### F. shallow root 的 `encoded_state_bytes` 所有权（代码确定，影响未测）

- `ContainerStore::decode_gc` 保存 `shallow_bytes.clone()`；若它是导入输入的切片，会持有整个输入
  缓冲区，而统计只计其长度。`pruned_root` 另有缓存。需要 owner-drop/计数测试确认冷/热路径下输入
  何时释放，再决定是否改为拷贝紧凑副本或落盘。

### G. 只有 `spill_dir`、没有额度/水位线时（需要用户决定）

- 没有自动回收触发。需要先定义目录只是后端选择还是承诺自动冬眠；不要擅自加默认额度。

### H. `StrArena::shrink_spare_capacity` 的净效果（代码确定，未测）

- `BytesSlice` 持有旧缓冲区的 `Arc`，紧凑复制不一定释放旧分配；`arena_bytes` 只计长度，统计看不出。
  全局兜底路径仍会调用（现在有退避）。需要分配计数证明净收益，否则只保留在显式 `reclaim_memory`。

### I. WASM 短字符串绑定（代码确定，收益未测）

- `js_str_to_string_checked` 总是多一次 `as_string()` 边界往返。用户已决定：**不接受**任何可绕过
  分配前检查的私有/后备导出。只能在保持所有入口（insert/insertUtf8/splice/update/updateByLine，
  nodejs/web/bundler/browser/base64）检查的前提下研究更快路径；必须 `CI=true pnpm release-wasm`
  并给出可核对的 A/B。

### J. 大批次 `importBatchDetailed` 元数据（代码确定）

- updates 元数据通过 `decode_updates` 完整解码全部 change（`fast_snapshot.rs` 的
  `decode_updates_blob_meta_from_body`），之后导入再拷贝、再解码一次。可研究只解析块头生成元数据，
  块体校验留到逐包导入；保持有界驻留、排序和逐包错误。

### K. JSON / checkout / 无预算性能 A/B（未完成）

- JSON 热缓存：无新证据。checkout 的 CID 探测：冷块首次探测已插入 `mem_parsed_kv`，重复查询收益
  **猜测**较小，先测量。无预算 +6.5% 仍来自复用的旧 rlib，需要重建各层与 main 的可核对基线
  （B4、JSON 冷/热、大混合文档、checkout、导出，时间与内存分开）。

### L. 其它线索（猜测）

- 约 9 MB 文本文档 `memoryStats().totalBytes` 约 228 MB，疑似状态估计大幅高估，未分析。
- 盒装载荷记账是保守估计：与状态共享的值按每个 Op 再计一次，可能让额度更早触发；如需更精确需定义
  “唯一实际 heap”的口径。
- 两个句柄在两线程并发 drop 的 Loom 模型超过 10 分钟未完成，目前只有原生压力测试覆盖。

## 最终汇报要求

逐项给出：已修/未修、复现或猜测、实际命令与结果（不能复用旧结果冒充新 HEAD）、性能与内存的
A/B（可核对的 source/head/binary）、仍需用户决定的语义、提交 SHA、新 draft PR 链接与 base/head。
