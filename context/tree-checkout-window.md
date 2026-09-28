# Tree Checkout Window

Verified against code 2026-09-28.

`TreeDiffCalculator` (`crates/loro-internal/src/diff_calc/tree.rs`) computes
tree diffs for checkouts and for imports in `DiffMode::Import`. It keeps one
`TreeCacheForDiff` per tree in the history cache (`TreeOpGroup` in
`history_cache.rs`): the tree's move ops of `current_vv`, grouped by target.

## Invariant

Each cached op carries `effected`: whether the cycle check let it apply
(`TreeCacheForDiff::apply`). That flag depends on every op ordered before it
by (lamport, peer), so the cache must always hold ops applied in that order. A
node's parent is its last effected op (`get_parent_with_id`).

## Transitions

`checkout` (moves the cache to `from` without diffs) and `checkout_diff`
(records `TreeDeltaItem`s from `from` to `to`) use the same window:

1. `min_lamport_of_version_diff(a, b)`: the smallest lamport among ops that are
   in exactly one of the two versions. Lamports grow along a peer's ops, so it
   reads only the first op of each per-peer span of the version-vector
   difference, O(peers).
2. Retreat every cached op at or above it, newest first. `checkout_diff` emits
   the inverse of each effected op.
3. Apply every op of the target version at or above it, oldest first.

Ops below the window are in both versions and stay as they are. Both passes
walk `TreeOpGroup::ops` (ordered by `IdLp`), which also holds the seeded
shallow-root entries, so they never scan nodes that did not change.

## Why not the replay base

Until 2026-09-28 the window started at the change-start lamport of
`find_replay_base(from, to)`. In `Checkout` mode that base is the meet even
when the meet is not a critical version (`MeetAsBase::Valid`), and an op
concurrent with the meet can have a lower lamport than the meet. Example
(`crates/loro/tests/tree_checkout_path.rs`): `0@3` creates a node at lamport 1
and is concurrent with `2@2` (lamport 2); checking out from `[3@2]` to
`[1@1]` used the meet `[2@2]`, the window started at 2, and the creation was
never retreated. In the other direction the forward pass skipped a node's
creation but applied its child's, and `TreeState::apply_diff_and_convert`
panicked on the child's `Create` (with a subscriber; without one the state was
wrong). `docs/critical-version-spec.md` Q7 had argued the window was safe.

The cache's own `checkout` had the same problem with a window taken from the
target frontiers.

## Shallow documents

The cache of a shallow document starts at the shallow root, seeded with the
root's nodes (`record_shallow_root_state`). `TreeCacheForDiff::init_current_vv`
sets `current_vv` to the root version before the first transition, so the
window never reaches trimmed ops. The seeding itself only covers the seeded op
IDs, which is why `current_vv` has to be replaced.

## Tests

- `crates/loro/tests/tree_checkout_path.rs`: the two repros.
- `crates/fuzz/tests/checkout_path.rs`: random concurrent edits on all
  container kinds, every ordered pair of versions (including heads no peer
  had), compared with a document that imported only that version's changes,
  with and without a subscriber (events are mirrored). Wider sweeps:
  `CHECKOUT_PATH_SEEDS=0..300 cargo test --release -p fuzz --test checkout_path`.
- `crates/loro/tests/issue.rs`: `checkout_across_non_critical_meet_stays_canonical`
  and `checkout_with_low_lamport_concurrent_branch_stays_canonical`.
