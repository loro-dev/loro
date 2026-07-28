# Diff Calc Replay Base Context

Verified against code 2026-07-28.

Every `checkout`, `fork_at`, `import`, and `revert_to` funnels through
`DiffCalculator::calc_diff_internal`, which first asks the DAG for a *replay
base* and then replays history forward from it. The base is not just a
correctness detail — it decides **which containers get a diff calculator at
all**, so a base that is too early is a document-sized performance cliff, not a
small constant. This article exists because that coupling is invisible from
either side on its own.

## Two-Hop Answer

- [crates/loro-internal/src/dag.rs](../crates/loro-internal/src/dag.rs):
  `find_common_ancestor` / `_find_common_ancestor` compute the base and the
  `DiffMode`.
- [crates/loro-internal/src/oplog.rs](../crates/loro-internal/src/oplog.rs):
  `OpLog::iter_from_lca_causally` clamps the base to the shallow root and yields
  the changes to replay.
- [crates/loro-internal/src/diff_calc.rs](../crates/loro-internal/src/diff_calc.rs):
  `calc_diff_internal` creates one calculator per container touched in that
  range; `DiffCalcVersionInfo::lca_vv` is handed to each calculator.

## Why An Early Base Is Expensive

The cost is multiplicative, not additive:

1. `iter_from_lca_causally` walks every change between the base and the merged
   version, so an empty base walks the entire oplog.
2. Every container touched anywhere in that walk gets a calculator, so an empty
   base wakes up every container in the document.
3. Each of the richtext/list/movable-list calculators independently decides to
   rebuild (see below), and `replay_container_ops_from_empty` re-walks the
   *whole* oplog filtering for its own `ContainerIdx`.

So an empty base costs `O(containers x ops)`. `crates/examples/examples/fork_at_many_containers.rs`
is the harness for this shape; at 1600 containers / 194k ops the difference
between an exact base and an empty one was 1.4ms vs 268ms for `checkout`, and
the ratio grows with document size (loro-dev/loro#1056).

## The Conservative Fallback

`_find_common_ancestor` sets `has_unmatched_branch` when a branch of its walk
reaches a root, or drains the queue, without ever meeting the other side. It
then throws away the ancestor it computed and returns `Frontiers::default()`.

This is deliberate: some calculators need a base that precedes every branch
whose operations can affect positions. It is *not* needed when `ans == right`
(the target is already in the source's past, i.e. an ordinary `checkout` or
`fork_at` backwards). There, every id of `right` turned out to be reachable from
`left`, so `right` is provably the maximal common ancestor.

That direction is also the safe one for the trackers: `from_vv` strictly
contains `to_vv`, so the retreat is never empty and
`RichtextDiffCalculator`/`ListDiffCalculator`/`MovableListDiffCalculator` take
their `should_rebuild` path regardless of the base.

The symmetric relaxation (`ans == left`, fast-forwarding past a concurrent
branch) is **not** safe. It has no retreat to force a rebuild, and leaning on
`mark_source_not_in_op_context` alone regresses `undo_tree` in
`crates/fuzz/tests/test.rs`. If you are tempted to widen the condition, run that
target first — the failure is silent in the unit tests.

## Which Calculators Care About The Base

- `MapDiffCalculator`, `TreeDiffCalculator` (Crdt mode), `CounterDiffCalculator`:
  resolve from `from_vv`/`to_vv` plus the history cache. Base-independent.
- `RichtextDiffCalculator`, `ListDiffCalculator`, `MovableListDiffCalculator`:
  hold an incremental tracker. They fall back to a full rebuild from CRDT ids
  when `has_retreat`, when `lca_vv != from_vv`, when the doc is shallow, or when
  `DiffCalculator` called `mark_source_not_in_op_context` because some replayed
  op's own context did not include the source version.

`is_right_greater` is forced to `false` on every unmatched-branch path, so these
cases always run in `DiffMode::Checkout` — the general mode — never `Linear` or
`ImportGreaterUpdates`.

## Shallow Documents

`OpLog::iter_from_lca_causally` clamps a base that points before
`shallow_since_vv` up to `shallow_since_frontiers`, because trimmed changes
cannot be replayed. `has_trimmed_history_deps` keeps `_find_common_ancestor`
from clearing `ans` when it already depends on trimmed history, since the clamp
would undo it anyway.

Note `fork_at` on a shallow document returns `LoroError::NotImplemented` from
`encode_snapshot_at`, but only *after* `_checkout_without_emitting` has already
moved the document to the target version — so the checkout cost is paid before
the error surfaces.
