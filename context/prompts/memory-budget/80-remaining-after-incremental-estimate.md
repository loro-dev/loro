# 给 agent：内存预算剩余问题（接 perf/memory-budget-remaining 继续）

Verified against code 2026-11-16（基于 `perf/memory-budget-remaining`）。

本文件自包含，取代 [70](70-remaining-after-followup-fixes.md) 作为当前待办清单。

## 起点与交付方式（必读）

- 仓库 `loro-dev/loro`。先 `git status --short --branch`，读根 `AGENTS.md`、
  `crates/loro-internal/AGENTS.md`（涉及 WASM 再读 `crates/loro-wasm/AGENTS.md`）、
  `context/memory-budget.md`。
- 基线分支：`perf/memory-budget-remaining`（draft PR，base 为
  `perf/memory-budget-followup-fixes`，即 #1304）。若已有以它为 base 的后续栈，接在最新 tip 上：
  `gh pr list --repo loro-dev/loro --search "base:perf/memory-budget-remaining" --json number,headRefName,url`。
- 从该 tip 在隔离工作树创建新分支，完成后推送并创建 **draft** PR，`--base` 填该 tip 的分支名。
  禁止以 `main` 为 base，禁止推送/覆盖 #1296/#1299/#1300/#1301/#1304 的分支或本分支，禁止合并或部署。
- 保留：硬额度、失败原子回滚、冷状态降级、流式导出；“新额度从下一笔事务生效、保留已有未提交编辑”。
- 共享机器：`timeout <秒> flock /root/e2ee-review/test.lock <cmd>`，`CARGO_PROFILE_DEV_DEBUG=0`；
  长 fuzz、浏览器矩阵先询问。发布行为变化加 changeset；改代码同时更新 `context/memory-budget.md`。

## 已完成（perf/memory-budget-remaining，勿重复）

1. **B 原生全局统计随文档数线性增长**：`DocStatsDirty` + `GlobalMemoryPool::dirty_docs`，全局估算只重算
   脏文档（见 `context/memory-budget.md`“Incremental native global estimate”）。1000 个空闲文档时
   `try_commit` ~880 → ~3.3 µs、`commit` ~300 → ~5.3 µs，1/100/1000 文档持平
   （`crates/loro/examples/global_budget_scaling_bench.rs`）。debug 构建每次估算都把干净文档的缓存与全量重算比对。
2. **D 的一部分**：单文档软回收退避（`SoftBackoff`，与全局共用）。KV flush 整表重写本身未改。
3. **F**：浅快照导入不再让整个输入缓冲区常驻（复制 shallow/state 两段，op log 段很小时保持零拷贝）。
4. **H**：已测，`shrink_spare_capacity` 在本地文本输入场景净释放约 70 KB，保留现状（无代码变更）。

## 待办

每项标注：**复现**＝有运行证据；**代码确定**＝读码确定、未运行复现；**猜测**＝待证实。

### A. 导入回滚自身失败时的原子性（已修：`perf/import-undo-log`，状态侧 undo log）

见 memory-budget.md “Import rollback without reading history”。下一步（用户已定）：用同一套 undo log
重写 UndoManager，去掉 undo/redo 中的 checkout，行为必须与现在完全一致（现有测试不改 + 新旧差分测试）。
难点：日志是提交时的位置，需要在本地提交时转成那一刻的逆向 `DiffBatch`（A_i）；B_i 改用栈里已合成的远端 diff
是否与 checkout 等价需要差分测试证明（#1245、#1228 曾因合成有损出错）。

### B'. 增量全局估算的后续（代码确定）

- 新的标脏来源必须遵守 `DocStatsDirty` 的规则（持有对应锁时或变更之后标脏），否则全局估算少算；
  debug 断言会在单线程测试中暴露遗漏。
- debug 构建的校验是 O(文档数)；如果 debug 下的多文档测试变慢，可改为抽样。
- `pnpm test-loom` 未在本分支运行（`dirty_docs` 是 parking_lot 锁，内部没有 loom 操作）；应补跑。
- 两个句柄在两线程并发 drop 的 Loom 模型仍未完成（见 70 的 L）。

### C. WASM 物理页额度语义（已修，C2：`perf/memory-budget-decisions`）

`tryReclaimGlobalToBudget` 与 `tryCommit` 同规则；JS 统计新增 `wasmLinearMemoryExceeded`。

### D. KV flush 整表重写（复现）

同 70 的 D，去掉“每文档 Stage 1 无退避”（已修）。分层 LSM、墓碑、流式合并仍待做。

### E. 落盘 IO 错误（已修：`take_spill_error`、读回块校验 checksum；读失败保持 panic，见 memory-budget.md）

### F'. 浅快照的剩余保留（猜测）

- op log 段很小（< (state+shallow)/8）时不复制，最多 op log 段大小未计入；`pruned_root` 缓存未测。
- 复制 state 段才释放的具体持有者（哪些容器保留了 state 段的切片）未定位。

### G. 只有 `spill_dir`、没有额度/水位线时（已定 G1：只是后端选择，`set_memory_budget` 打 warn）

### I. WASM 短字符串绑定（代码确定，收益未测）

同 70 的 I（用户已决定：不接受可绕过分配前检查的私有/后备导出）。

### J. 大批次 `importBatchDetailed` 元数据（代码确定）

同 70 的 J。

### K. JSON / checkout / 无预算性能 A/B（未完成）

同 70 的 K。本分支只做了 `richtext_typing_bench plain` 与 `memory_budget_commit_bench` 的交错 A/B
（无可测回退，噪声内），未重建各层与 main 的完整基线。

### L. 其它线索（猜测）

同 70 的 L（约 9 MB 文本的 `totalBytes` 约 228 MB 疑似高估；盒装载荷保守记账）。

## 最终汇报要求

逐项给出：已修/未修、复现或猜测、实际命令与结果（不能复用旧结果冒充新 HEAD）、性能与内存的
A/B（可核对的 source/head/binary）、仍需用户决定的语义、提交 SHA、新 draft PR 链接与 base/head。
