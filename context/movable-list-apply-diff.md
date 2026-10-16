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

`src/undo/movable_list.rs` captures each continuous undo span's before/after
elements during the checkouts already used to calculate its inverse. The
capture includes element ids, position ids, value ids, and values, before
positional transformation can lose them (loro-dev/loro#1185). It captures lists
touched by the span, plus the full-state sources of revived list children.
Other container types keep their existing inverse calculation and transformation.

The private plan reverses spans newest first against the current list. It
deletes inserted elements by identity wherever they moved, restores removed
elements with fresh ids, and issues native moves/sets for surviving elements.
`UndoManager::element_remap` links removed ids to their recreated ids so older
steps, redo, and saved neighbour references can find them. A move/set whose
element was remotely deleted does not recreate it. This does not change the
manager's existing no-op skipping policy (the separate #1183).

Position restoration prefers the nearest surviving original left neighbour;
otherwise it uses the nearest surviving original right neighbour, then index
zero. The left neighbour wins when the old neighbours cross. Restoration goes
in original order, so restored runs anchor each other; right anchors exclude
elements that the same inverse has yet to move. Remote insertions in the old
gap follow the restored element. A removal already made ineffective by a
concurrent move does not insert another copy.

The planner uses an ordered model with fractional keys to avoid shifting or
scanning the whole list per edit. Live element lookup uses the existing
`MovableListState::get_index_of_elem` index. A trimmed public value delta is
used by the existing whole-batch unknown-container preflight, but only the
native plan is applied. Its unexpected application failure remains an
invariant failure, rather than being logged as a successful partial undo.
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
- `undo_internal` logs and ignores an `Err` from `_apply_diff`, but a panic
  escapes, and in WASM a panic traps. Until 2026-09 this handler deferred all
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
