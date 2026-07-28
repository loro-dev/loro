# Context Discoverability Gaps (backlog)

Append a line when you discovered something important the hard way but could not
fix the docs in that change.

Format:
`YYYY-MM-DD | <question an agent would ask> | <answer + file anchors> | why it was hard | suggested home`

2026-07-28 | Why is `checkout`/`fork_at` on a single large text container still ~2x slower than 1.12? | `should_rebuild` in `RichtextDiffCalculator::calculate_diff` and `ListDiffCalculator::calculate_diff` (crates/loro-internal/src/diff_calc.rs) rebuilds the tracker from empty via `build_full_crdt_tracker` whenever `has_retreat` is set, i.e. on *any* backwards checkout, instead of only when the incremental tracker cannot express an op's context. Added by b81abfce for #974. Measured on B4 (120k ops, median of 5): checkout 7.1ms -> 22.1ms, fork_at 19.6ms -> 69.8ms vs the pre-b81abfce baseline. Not fixed in #1056 because the same unconditional rebuild is what makes the `ans == right` narrowing in `find_common_ancestor` safe; tightening it needs its own correctness argument and a `crates/fuzz` run (`undo_tree` is the canary). | The cost is invisible from `find_common_ancestor` alone - it only shows up when you measure a single-container document, where the #1056 multi-container win does not apply. | context/diff-calc-replay-base.md
