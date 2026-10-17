# Movable List `apply_diff` and Undo

Verified against code 2026-10-17.

`MovableListHandler::apply_delta`
(`crates/loro-internal/src/handler/movable_list_apply_delta.rs`) turns a list
delta into local movable-list ops. `LoroDoc::apply_diff`, `revert_to`, and
`UndoManager` undo/redo reach the diff application loop through
`LoroDoc::apply_diff_with_movable_undo`. Public `apply_diff` and `revert_to`
still use value deltas. Undo has a private identity plan for lists that keep
the same container lifetime, as described below.

## Undo identities and anchors

`src/undo/movable_list.rs` derives affected element ids from the span's ops;
delete position ids resolve to their original insertion or move. It captures
those elements during the checkouts already used to calculate its inverse. The
capture includes element ids, position ids, value ids, and values, before
positional transformation can lose them (loro-dev/loro#1185). It captures lists
touched by the span, plus the full-state sources of revived list children.
Other container types keep their existing inverse calculation and transformation.

The private plan reverses spans newest first against the current list. It
deletes inserted elements by identity wherever they moved, restores removed
elements with fresh ids, and issues native moves/sets for surviving elements.
`UndoManager::element_remap` links removed ids to their recreated ids so older
steps, redo, and saved neighbour references can find them. A move/set whose
element was remotely deleted does not recreate it. A consumed no-op returns
`false` and leaves other items for later calls, following #1202's
[history boundary rule](undo-history-boundaries.md). Undoing an insertion
whose element moved remotely is now a real identity delete, not a no-op.

Position restoration prefers the nearest surviving original left neighbour;
otherwise it uses the nearest surviving original right neighbour, then index
zero. The left neighbour wins when the old neighbours cross. Restoration goes
in original order, so restored runs anchor each other; right anchors exclude
elements that the same inverse has yet to move. With a surviving left anchor,
remote insertions in that gap follow the restored element. With only a right
anchor, restoration goes immediately before it, after any new prefix.
A removal already made ineffective by a
concurrent move does not insert another copy.

Before snapshots also capture the nearest original neighbours. Their survival
is checked by id in the current state. If a remote deletion removed a
candidate, `complete_anchors` extends only that search in the historical view,
doubling the number of candidates until it finds the nearest survivor. It
does not copy a live-id table or unrelated list values. The ordered model
contains only affected elements and these anchors. Insert/move/set use indexed
lookups; deletion positions older than a shallow root retain the full-state
fallback because their original operations have been trimmed. A full child
revival necessarily retains its full-state source.

`NativePreflight` records actual child creations/removals for the existing
whole-batch unknown-container check. Its private list entry advertises
creations, not a public positional delta; native edits are applied separately.
No native edit runs before the whole batch passes that check. The transaction
guard remains held from capture/preparation through all native edits, which
use the handlers' `*_with_txn` methods. A waiting writer cannot invalidate an
element or anchor in the former unlock/relock window. Stale-plan errors are
returned as `LoroError`, without an `expect` or successful partial-undo log.

A remotely revived original can coexist with its undo-created replacement.
Several historical ids may therefore resolve to one current alias. Placement
is classified per original entry; a missing move/set without an insertion
value is skipped, never unwrapped or revived.
Lists whose parent changes their lifetime retain the existing child revival
diff path. The saved full-state source, adjusted for earlier spans of the same
undo group, supplies aliases for the cloned elements after that replay. This
keeps older list steps and their anchors usable after a parent is recreated.
An unaligned recreation retains the previous value-delta path instead of
treating unmapped elements as remotely deleted. No public delta or persisted
encoding changes.

## Contract

- An inserted container value that matches a child in a deleted range of the
  same delta is applied as a move, so the child keeps its id and content. The
  lookup follows `container_remap` for children recreated by an earlier
  undo/apply_diff.
- Any other container value creates a new child and records
  `old id -> new id` in `container_remap`. Later diffs in the batch for the old
  id go to the new child.
- An insert with `from_move` whose child still exists elsewhere in the list is
  skipped. Undo transforms can leave such inserts behind.
- Scalars in public value deltas have no identity. Deleting and re-inserting a
  scalar through `apply_diff` is a delete and a new insert, even when `is_move`
  is set. Undo's private plan preserves their identity.

## Algorithm

1. Plan: collect delete ranges in original indices and decide which deleted
   children are claimed by the inserts.
2. Delete unclaimed elements right to left, so original indices stay valid.
3. Walk the delta with a live `cursor`. Each inserted value goes right after
   the previously placed target element. `live[cursor..]` is the unplaced
   claimed children of the current delete gap, followed by the unmoved
   original elements at or after the delta position. A retain skips both. A
   claimed child found before the cursor was left behind by an earlier retain
   and moves to `cursor - 1`.

Claimed children are located with `get_child_index` (O(log n)). Every step
costs O(log n) plus the op itself.

## Pitfalls

- Separate existing boundary (also reproduced on `3827945`): start with a
  nested movable list `[7]`, record an earlier push of `8`, then group a push
  of `9`, import an unrelated remote map edit, and delete the parent list
  entry. Undoing the group restores an empty child instead of `[7, 8]`.
  This is the generic composition of child revival across split spans, not
  #1185's concurrent move/removal inversion. The identity repair keeps that
  parent-revival application path; it does not rewrite group composition.
- `undo_internal` rolls back application errors and returns them; the failed
  item and both identity maps are preserved for retry
  ([undo-atomicity.md](undo-atomicity.md)). A panic on an internal invariant
  still escapes, and in WASM a panic traps. Until 2026-09 this handler deferred all
  deletes and tracked moved slots with index arithmetic. The arithmetic
  underflowed or produced out-of-range `mov` calls behind an `unwrap()`: a
  swap of two children lost one of them, and undo/redo could panic with
  `OutOfBound`. Keep index bookkeeping checked by the random tests below.
- The public `ListDiffItem` → `DeltaRope` conversion merges adjacent deletes,
  so Rust callers never see `delete 1, delete 2`. loro.js walks its public
  delta array item by item and has to handle split deletes itself.
- Moves are not minimal. `delete 3, insert [b, c, a]` moves `b` and `c`
  instead of only `a`. Deltas from `LoroDoc::diff` only move elements that
  really moved, so they produce the minimal number of ops.

## Tests

- `crates/loro/tests/movable_list_apply_diff.rs`: targeted cases, a seeded
  differential test against `LoroDoc::diff`, random deltas with undo/redo
  across every step, and a two-peer no-panic test.
- `crates/loro/tests/movable_list_undo.rs`: #1185's two reproductions,
  equal-valued identities, neighbour motion/deletion, remote deletion, child
  identity, grouped spans, recreated ids/parents, repeated undo/redo, all 144 pairs of
  four-element concurrent moves, convergence and snapshot/update round trips.
- `crates/loro-wasm/tests/movable_list.test.ts`: the JS `applyDiff` cases.
- `crates/loro/tests/perf_movable_list_apply_diff.rs` (ignored): 100k-element
  timings for `apply_diff` and undo.
- `crates/loro/tests/movable_list_undo_random.rs`: three peers, scalar/map
  values, random sync/undo/redo, convergence and snapshot/update round trips;
  `LORO_UNDO_RANDOM_SEEDS=1500` runs the extended corpus, including 203/1340.
- `crates/loro/tests/movable_list_undo_concurrent.rs`: a writer joins the
  transaction wait queue during undo's historical checkout; the same manager
  and document remain usable. `LORO_UNDO_CONCURRENT_ROUNDS=3000` extends it.
- `undo::movable_list::tests::small_edits_keep_snapshot_and_model_sizes_bounded`
  bounds captured/model elements independently of list length, for moves and
  front removals. Revival detection checks length before capturing full state.
- `crates/loro/tests/movable_list_undo_perf.rs`: optimized 10k/100k-element
  single-move undo+redo medians and explicit performance budgets.
