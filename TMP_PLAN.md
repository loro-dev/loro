# Text Checkout Performance Plan

本计划用于追踪 text checkout 性能优化。范围限定为前五项，不考虑为 text insert 缓存或编码 origin anchors、container/frontier text checkpoints 等长期缓存方案，因为这类缓存容易遗漏 underwater 数据和隐藏状态。

## 目标场景

- 多协作者异步编辑，peer 数量最多上千。
- 类 Obsidian/代码的 plain text：大文档、局部编辑、长历史、频繁 checkout。
- 类 Notion 的 rich text：样式范围、并发 mark、订阅事件转换。
- 高冲突场景：大量 peer 在同一位置或相邻位置插入。
- detached/checkout-to-latest 往返、离线分支合并后切换版本。

## 阶段 1: 建立 Text Checkout 专用 Benchmark

- [x] 新增 benchmark 覆盖 text checkout，而不是只覆盖 apply/import。
- [x] 场景 A：1000 peer 随机位置小编辑，随机 checkout 到历史 frontiers。
- [x] 场景 B：1000 peer 同一位置或相邻位置插入，验证 future sibling 扫描成本。
- [x] 场景 A2：1000 peer 顺序多 peer 编辑，causal VV 宽度增长到 1000，验证 per-node wide VV 成本。
- [x] 场景 C：plain code/markdown，大文档、长事务和 one-op-one-txn 两种历史形态。
- [x] 场景 D：rich text，样式 start/end、重叠 mark、删除样式范围。
- [x] 场景 E：有订阅与无订阅各跑一组，拆出 event conversion 成本。
- [x] 在 benchmark 中分段计时：`frontiers_to_vv`、`diff_calc`、`RichtextTracker::checkout/diff`、`RichtextState::apply_diff`、event conversion。
  - 当前输出 `frontier_prepare`、`frontiers_to_vv`、`diff_calc`、`richtext_tracker_checkout`、`richtext_tracker_diff`、`richtext_delta_build`、`richtext_insert_future_scan`、`state.apply_diff`、`emit_events`。
- [x] 给 benchmark 输出保留关键规模参数：peer 数、change 数、text 长度、text rope node 数、style node 数、diff item 数。
  - 当前输出 peer/change/text/version、VV/frontier 宽度、diff container 数、richtext tree node/chunk、text chunk、style anchor、style range tree node/chunk。

验收标准：

- [x] 能稳定复现当前 text checkout 的主要热点。
- [x] 能区分 VV 宽度、causal 切换、CRDT rope 插入扫描、state apply、event conversion 的占比。

## 阶段 2: 降低 Per-change VersionVector 成本

当前疑点：

- `OpLog::iter_from_lca_causally` 每个 DAG node 都构造/清空/扩展完整 `VersionVector`。
- 1000 peer 时，即便每个 change 很小，也会有 O(changes * peers) 的成本。

计划：

- [x] 将 `iter_from_lca_causally` 输出的 per-node VV 从完整复制改成轻量上下文。
- [x] 优先尝试用 `(base ImVersionVector, current peer end counter)` 或等价 view 表达当前因果版本。
- [x] 为 `RichtextTracker::checkout` 增加直接消费 retreat/forward spans 的内部接口，避免为了 diff 两个 VV 再扫描所有 peer。
- [x] 保持 public API 不变，所有改动限制在 internal diff calc/tracker 路径。
- [x] 加回归测试覆盖多 peer、并发分支、checkout 前后状态一致。

验收标准：

- [x] 1000 peer 小 change 场景中，`diff_calc` 时间随 peer 数增长明显降低。
- [x] 现有 checkout/import/fuzz 相关轻量测试通过。

## 阶段 3: 可比版本走 Forward/Linear Fast Path

当前疑点：

- persist `DiffCalculator` 会把 diff mode 强制为 `Checkout`，导致可比版本也走更慢、更通用的 CRDT checkout 路径。
- 对 `from < to` 或 checkout-to-latest，很多时候可以使用更便宜的 forward/linear/import-greater 逻辑。

计划：

- [x] 明确区分目标：真实历史 checkout 与单调前进 checkout-to-latest。
- [x] 在安全条件满足时，让 text diff 保持 `Linear` 或 `ImportGreaterUpdates` 路径。
- [x] 如果复用 persistent richtext tracker 会破坏缓存状态，则选择失效 tracker 或延迟重建，而不是强制所有路径进入 `Checkout`。
- [x] 覆盖 detached 状态、checkout-to-latest、多容器 revive、订阅事件。

验收标准：

- [x] checkout-to-latest 在可比版本场景中避开 CRDT tracker 的双 checkout。
- [x] 不改变 public checkout 语义和事件语义。

## 阶段 4: 优化 Plain Text Apply Diff 和 Event Conversion

当前疑点：

- `RichtextState::apply_diff` 已有 plain text choppy rebuild fast path，但 no-style/no-event 情况还可以更直接。
- 有订阅时 `apply_diff_and_convert` 会生成 external text delta，style/event index 转换会放大成本。

计划：

- [x] 拆出 no-style/no-event 的 plain text apply fast path。
- [x] 为 `drain_by_entity_index` 增加不需要 event index 和 affected style range 的内部路径。
- [x] 优化单 leaf 删除与插入，避免重复 query 和 cursor conversion。
- [ ] 对有订阅场景，减少 `style_delta.compose` 次数，能批量 compose 时批量处理。
- [x] 保持内部不变量：无效外部输入返回 `Err`，内部状态不一致继续 fail-fast。

验收标准：

- [x] plain text 无订阅 checkout apply 成本下降。
- [x] 有订阅场景外部 event delta 保持一致。
- [x] rich text 样式事件测试不回退。

## 阶段 5: 优化同位置高并发插入扫描

当前疑点：

- `CrdtRope::insert` 在当前位置向右扫描 future spans，以确定并发插入顺序。
- 多 peer 同一位置插入时，future sibling 扫描可能接近二次行为。

计划：

- [x] 用 benchmark 场景 B 先确认瓶颈规模和触发条件。
- [x] 研究为相同 `(origin_left, origin_right)` 或同 active position 的 future group 建辅助索引。
- [x] 确认暂不引入需要随着 leaf split、future/active 状态变化维护的持久索引，先用局部 fast path 避免错误顺序风险。
- [x] 先实现最小内部索引、局部缓存或局部 fast path，只覆盖同一位置冲突热点。
- [x] 加测试覆盖 peer id 排序、不同 right parent、future spans、delete/retreat/forward 后再次插入。

验收标准：

- [ ] 同位置 1000 peer 插入 checkout/import 成本从接近二次趋势降到接近 `N log N` 或更好。
- [ ] Fugue ordering 与现有测试/fuzz 结果一致。

## 执行顺序

1. 先做阶段 1，避免没有基线就改热点。
2. 再做阶段 2，因为 VV 宽度是多协作者场景最确定的通用成本。
3. 接着做阶段 3，优化 checkout-to-latest 和单调前进版本切换。
4. 然后做阶段 4，降低 state apply 和事件转换成本。
5. 最后做阶段 5，它对高冲突文本最关键，但实现风险最高。

## 每阶段记录

每完成一个阶段，在这里补充：

- commit 或 patch 范围：
- benchmark 命令：
- before/after 数据：
- 发现的新瓶颈：
- 是否需要调整下一阶段：

### 阶段 1 记录

- patch 范围：`loro.rs` 增加 `test_utils` only `CheckoutProfile`/`checkout_with_profile`；新增 `benches/text_checkout.rs`；`Cargo.toml` 注册 bench。
- benchmark 命令：`LORO_TEXT_CHECKOUT_PROFILE=1 cargo bench -p loro-internal --features test_utils --bench text_checkout`。
- 参数：`LORO_TEXT_CHECKOUT_PEERS`、`LORO_TEXT_CHECKOUT_BASE_LEN`、`LORO_TEXT_CHECKOUT_CHANGES` 可覆盖默认规模。
- 验证命令：`cargo check -p loro-internal --features test_utils --bench text_checkout`；small smoke：`LORO_TEXT_CHECKOUT_PROFILE=1 LORO_TEXT_CHECKOUT_PEERS=8 LORO_TEXT_CHECKOUT_BASE_LEN=128 LORO_TEXT_CHECKOUT_CHANGES=16 cargo bench -p loro-internal --features test_utils --bench text_checkout -- --warm-up-time 0.1 --measurement-time 0.1 --sample-size 10`。
- 增量补充：rich text subscribed mark 场景、rich text unmark/style deletion 场景、wide-causal sequential multi-peer 场景、richtext/style range BTree node/chunk 统计、RichtextTracker checkout/diff/delta build 分段。
- before/after 数据：阶段 2 已记录 wide-causal 数据；阶段 1 作为基准与埋点保留。
- 发现的新瓶颈：wide-causal 场景显示 `RichtextTracker::checkout` 的 causal target 扫描比 per-node VV materialization 更重。
- 是否需要调整下一阶段：rich text 删除样式范围和 rope/style node 数已补；阶段 2 已增加 causal view 与单 frontier fast path。

### 阶段 2 记录

- 前置 profile：在 `iter_from_lca_causally` 的 per-node VV materialization 位置记录 `avg_causal_vv_materialize`、`causal_vv_materialize_calls`、`max_causal_vv_width`。
- 目的：先把 `clear + extend_to_include_vv` 的 O(node * peer) 成本从 `diff_calc` 中拆出来，再做轻量 VV/view 优化。
- 首个优化：`RichtextTracker::_checkout` 不再 clone 目标 `VersionVector` 到 `current_vv`，改为复用 diff 出来的 retreat/forward spans 增量更新当前 VV。这个不解决 `iter_from_lca_causally` per-node materialization，但先移除 tracker checkout 内部的 O(peer) clone。
- 第二个优化：`iter_from_lca_causally` 不再为每个 replayed change 清空并扩展完整 `VersionVector`，改为返回 O(1) clone 的 `ImVersionVector` 基底和 DAG deps frontiers；`DiffCalculator` 构造 `CausalVersion(base, peer_end, single_frontier_hint)` 传给 text/list tracker。
- 第三个优化：`RichtextTracker::checkout_causal` 直接从轻量 causal view 计算 spans；同时维护 `current_frontier_hint`，当 replay target 正好是刚应用过的单 frontier 时跳过 checkout span 扫描。这个覆盖线性/顺序多人编辑和同一事务连续 op；分叉、多 frontier、历史跳转仍走完整 causal checkout。
- 新增回归测试：`loro::test::text_checkout_wide_causal_multi_peer`，覆盖 32 peer 顺序编辑后前后 checkout。
- 验证命令：同阶段 1 的 `cargo check` 与 small smoke bench；`cargo check -p loro-internal`；`cargo test -p loro-internal tracker:: --features test_utils`；`cargo test -p loro-internal richtext --features test_utils`；`cargo test -p loro-internal checkout --features test_utils`；`cargo test -p loro-internal import --features test_utils`。
- 100 peer profile smoke：`plain/random-peer-checkout` 平均约 645us，`richtext_tracker_checkout` 平均约 51us，`max_frontiers_width=100`，`max_vv_width=101`。
- 100 peer wide-causal smoke：`plain/wide-causal-peer-checkout` 平均约 244us，`max_causal_vv_width=100`，`max_vv_width=101`。
- 1000 peer wide-causal before fast hint：平均约 5.13ms，`avg_diff_calc=4.90ms`，`avg_richtext_tracker_checkout=3.47ms`，`max_causal_vv_width=1000`。
- 1000 peer wide-causal after fast hint：平均约 1.61ms，`avg_diff_calc=1.39ms`，`avg_richtext_tracker_checkout=37.6us`，`max_causal_vv_width=1000`。
- 轻量 fuzz 验证：`cargo test -p fuzz random_fuzz_1s -- --nocapture`，2-site/5-site 的 6 个 1 秒 arbtest 随机用例通过。
- 未运行：libFuzzer targets；如继续合并前需要再决定是否跑 `cargo fuzz run all` 或 `crates/fuzz/fuzz` 的相关目标。

### 阶段 3 记录

- patch 范围：`LoroDoc::_checkout_without_emitting` 和 profile 版本在 `before < after` 时使用临时 `DiffCalculator::new(false)`，保留 `find_common_ancestor` 推导出的 `Linear` / `ImportGreaterUpdates`；历史/并发 checkout 继续使用持久 `diff_calculator` 的 `Checkout` 路径。
- 缓存策略：forward checkout 不复用持久 richtext tracker，避免把持久 tracker 切到 `Linear` mode 或污染历史 checkout cache；后续历史 checkout 若需要 tracker，会按现有 `all_vv` 检查重建。
- benchmark 增量：新增 `code/checkout-to-latest-linear`，每次先 checkout 到旧版本，再只计量 checkout 回 latest 的耗时；profile 输出 `forward_diff_calculator_samples`。
- smoke 命令：`LORO_TEXT_CHECKOUT_PROFILE=1 LORO_TEXT_CHECKOUT_PEERS=50 LORO_TEXT_CHECKOUT_BASE_LEN=1024 LORO_TEXT_CHECKOUT_CHANGES=128 cargo bench -p loro-internal --features test_utils --bench text_checkout -- code/checkout-to-latest-linear --warm-up-time 0.05 --measurement-time 0.1 --sample-size 10`。
- smoke 数据：平均约 65us，`avg_diff_calc=44.7us`，`richtext_tracker_checkout_calls=0`，`richtext_tracker_diff_calls=0`，`forward_diff_calculator_samples=640`。
- 新增回归测试：`loro::test::checkout_to_latest_linear_text_state_consistent`，覆盖 detached 旧版本 -> checkout_to_latest，验证文本内容、attached 状态和 `check_state_diff_calc_consistency_slow`。
- 验证命令：`cargo check -p loro-internal --features test_utils --bench text_checkout`；`cargo check -p loro-internal`；`cargo test -p loro-internal checkout --features test_utils`；`cargo test -p loro-internal richtext --features test_utils`；`cargo test -p loro-internal import --features test_utils`。

### 阶段 4 记录

- patch 范围：`InnerState` 增加 plain text 专用 `insert_text_chunk_at_entity_index` 和 `drain_plain_text_by_entity_index`；`RichtextState::apply_diff` 在无 style、plain text delta、无 event conversion 的路径上绕过 style range/event index 维护。
- 实现边界：仅当当前 state 没有 style、delta value 全是 text、且存在 edit action 时启用；rich text style anchor/range 继续走原通用路径。
- choppy rebuild：沿用原先 plain text rebuild 思路，但与 no-style 判定共用一次 delta 扫描；小 delta 仍走增量 apply，避免为局部编辑重建全文。
- 回滚过的尝试：最初在 direct insert 中维护 cursor cache，`checkout-to-latest-linear` smoke 反而从约 65us 退化到约 99us；改为 direct entity query + clear cache 后恢复。
- smoke 命令：`LORO_TEXT_CHECKOUT_PROFILE=1 LORO_TEXT_CHECKOUT_PEERS=50 LORO_TEXT_CHECKOUT_BASE_LEN=1024 LORO_TEXT_CHECKOUT_CHANGES=128 cargo bench -p loro-internal --features test_utils --bench text_checkout -- code/checkout-to-latest-linear --warm-up-time 0.05 --measurement-time 0.1 --sample-size 10`。
- smoke 数据：阶段 3 基准平均约 65.4us、`avg_state_apply=19.2us`；阶段 4 最终平均约 65.3us、`avg_state_apply=18.7us`。这个场景中主要收益很小，说明 forward diff 已经是主优化；但 no-style apply 路径现在避免了 style/event 相关维护成本。
- 验证命令：`cargo check -p loro-internal --features test_utils --bench text_checkout`；`cargo test -p loro-internal checkout --features test_utils`；`cargo test -p loro-internal richtext --features test_utils`；`cargo test -p loro-internal import --features test_utils`；`cargo check -p loro-internal`。
- 未完成：`style_delta.compose` 批量化还没做；这只影响有订阅/rich event conversion 的后续阶段 4 子项。
- 轻量 fuzz 验证：`cargo test -p fuzz random_fuzz_1s -- --nocapture` 通过。
- 未运行：libFuzzer targets；如合并前需要覆盖 checkout/import/state replay 的长时间模糊测试，还需要单独安排。

### 阶段 5 记录

- 首轮定位：`plain/same-position-peer-checkout` 在 300 peer 下先暴露的最大热点不是 rope 插入扫描，而是宽 frontier 的重复 `shrink_frontiers`。before：平均约 4.93ms，`avg_frontier_prepare=3.04ms`，`avg_diff_calc=1.77ms`。
- frontier 优化：`shrink_frontiers` 增加 same-deps fast path。去重后的 frontier DAG nodes 如果共享同一 deps，则它们互相并发，直接按原 lamport 降序返回，不做 ancestor walk；这不是长期缓存，不依赖 underwater 数据。
- 300 peer same-position after frontier fast path：平均约 1.78ms，`avg_frontier_prepare=37.8us`，`avg_diff_calc=1.65ms`。
- 1000 peer same-position after frontier fast path：平均约 16.6ms，`avg_frontier_prepare=240us`，`avg_frontiers_to_vv=450us`，`avg_diff_calc=15.85ms`。剩余主成本回到 replay/diff_calc。
- profile 增量：新增 `richtext_insert_future_scan`、scan calls、avg/max visited，用来隔离 `CrdtRope::insert` 内同 active position 的 future sibling 扫描。
- future scan 定位：1000 peer same-position 下，加入 profile 后平均约 20.56ms，`avg_richtext_insert_future_scan=1.83ms`，`richtext_insert_future_scan_calls=9674`，`avg_future_scan_visited=383`，`max_future_scan_visited=999`。
- future scan 优化：当 `in_between` 全部和待插入 span 具有相同 `origin_left/origin_right` 时，跳过通用 visited/right-parent 比较逻辑，直接按 peer 排序用 `partition_point` 找插入点；混合 right-parent 继续走原路径，并用 debug assert 固定同父 fast path 的 peer 有序前提。
- 1000 peer same-position after same-parent fast path：平均约 15.85ms，`avg_richtext_insert_future_scan=575us`，`avg_future_scan_visited=383`，`max_future_scan_visited=999`。
- 新增回归测试：`loro::test::checkout_same_deps_same_position_frontiers_text_consistent`，覆盖 32 peer 从同一 base 同位置插入后，用宽 frontiers checkout 到 base 再回 latest，并检查状态/diff consistency。
- 新增低层回归测试：`same_parent_future_spans_keep_peer_order`、`same_parent_future_spans_keep_order_after_retreat_forward`、`mixed_right_parent_future_spans_fall_back_to_general_ordering`，覆盖 peer id 排序、不同 right parent、future spans、delete/retreat/forward 后再次插入。
- 验证命令：`cargo test -p loro-internal richtext --features test_utils`；`cargo test -p loro-internal checkout --features test_utils`；`cargo test -p loro-internal checkout_same_deps_same_position_frontiers_text_consistent --features test_utils`。
- 验证补充：`cargo test -p loro-internal crdt_rope::test --features test_utils`。
- 未完成：还没有实现随 leaf split/future-active 状态维护的真正 sibling index；当前是低风险 fast path，因此不能把同位置 1000 peer 的扫描复杂度标为已经降到 `N log N`。
- 轻量 fuzz 验证：`cargo test -p fuzz random_fuzz_1s -- --nocapture` 通过。
- 未运行：libFuzzer targets；Fugue ordering 合并前应优先跑相关 `cargo fuzz` 目标。
