# Movable List `apply_diff` and Undo

Verified against code 2026-09-28.

`MovableListHandler::apply_delta`
(`crates/loro-internal/src/handler/movable_list_apply_delta.rs`) turns a list
delta into local movable-list ops. `LoroDoc::apply_diff`, `revert_to`, and
`UndoManager` undo/redo all reach it through `LoroDoc::_apply_diff` →
`Handler::apply_diff`. Undo is only a diff: `LoroDoc::undo_internal` computes
the inverse delta by checkout and replays it through this handler.

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
- Scalars have no identity. Deleting and re-inserting a scalar is a delete and
  a new insert, even when `is_move` is set.

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
- `crates/loro-wasm/tests/movable_list.test.ts`: the JS `applyDiff` cases.
- `crates/loro/tests/perf_movable_list_apply_diff.rs` (ignored): 100k-element
  timings for `apply_diff` and undo.
