# Tree Checkout Window

Verified against code 2026-11-12.

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

The retreat pass ends at `TreeCacheForDiff::max_lamport`: no cached op has a
greater lamport (`apply` and the shallow-root seeding raise it,
`retreated_from` lowers it to just below the window). The group holds the
whole history, so without that bound a checkout between two old versions
walked every later tree op and grew linearly with the history (about 3.8 ms
per checkout at 300k later moves). The forward pass ends at the target's max
lamport.

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

## Retained moves can change effectiveness (#1117)

Retreating only the ops absent from the target is also unsound, even when
all changed ops fall inside the window. In the issue's two-node, six-op
history (IDs are `counter@peer`):

1. `0@1` and `1@1` create n0 and n1 at the root.
2. `2@1` moves n1 under n0 (lamport 2).
3. Concurrently, `0@2` moves n0 to the root (lamport 2), then `1@2`
   moves n0 under n1 (lamport 3).
4. `3@1` moves n1 back to the root (lamport 3). It sorts before `1@2`.

At the latest version, both lamport-3 moves apply: n1 is at the root and
n0 is below it. At the target, all ops except `3@1` are present, and
`1@2` must be skipped because n1 is still below n0. The old diff path
retreated only `3@1` while keeping `1@2` and its latest-version
`effected=true`. Restoring n1's earlier parent then formed n0 ↔ n1, and
`is_parent_deleted` recursed until the stack overflowed. The cache-only
`checkout` transition likewise retained stale effectiveness flags.

PR #1152 (`6e294c87`) already fixed this root cause: retreat the **whole**
ordered suffix, including ops retained by both versions, and replay it for
the target. Here the window starts at lamport 3; `1@2` is retreated before
`3@1` and is then replayed and skipped. No parent query observes a cycle.
The exact issue test overflows on #1152's parent and passes on current main.

Cache ancestor and deletion queries now walk iteratively, just like the
materialized tree state. Depth uses constant call-stack space; a walk beyond
the number of cached nodes plus one (allowing an unknown parent to resolve
to `Unexist`) fails an internal invariant rather than hanging or returning
a fabricated placement. This is additional protection, not a substitute for
recomputing `effected` over the entire affected suffix.

## Shallow documents

The cache of a shallow document starts at the shallow root, seeded with the
root's nodes (`record_shallow_root_state`). `TreeCacheForDiff::init_current_vv`
sets `current_vv` to the root version before the first transition, so the
window never reaches trimmed ops. The seeding itself only covers the seeded op
IDs, which is why `current_vv` has to be replaced.

## Tests

- `crates/loro/tests/tree_checkout_path.rs`: the two window repros.
- `crates/loro/tests/tree_history_cycle.rs`: #1117's exact six ops, checked
  against `updates_till` replay and the target peer; fork, checkout round trips
  with/without a subscriber, both diff directions applied to independent docs,
  revert, snapshot-at, and shallow checkout round trips from two shallow roots.
  Each probe runs in a subprocess because stack overflow aborts.
- `diff_calc::tree::test`: a 16,000-node cache on a 1 MiB stack, and explicit
  rejection of corrupt parent cycles by both parent queries.
- `retreat_bound_follows_the_cached_version` (unit test in `tree.rs`) and the
  ignored `crates/loro/tests/perf_tree_checkout_old_versions.rs` (run with
  `--release -- --ignored`): the retreat bound.
- `crates/fuzz/tests/checkout_path.rs`: random concurrent edits on all
  container kinds, every ordered pair of versions (including heads no peer
  had), compared with a document that imported only that version's changes,
  with and without a subscriber (events are mirrored). Wider sweeps:
  `CHECKOUT_PATH_SEEDS=0..300 cargo test --release -p fuzz --test checkout_path`.
- `crates/loro/tests/issue.rs`: `checkout_across_non_critical_meet_stays_canonical`
  and `checkout_with_low_lamport_concurrent_branch_stays_canonical`.
