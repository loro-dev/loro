# Large Text Ops: Checkout in Linear Time

Verified against code 2026-11-12.

## The shape that was quadratic

The text tracker (`crates/loro-internal/src/container/richtext/tracker.rs`)
keeps two structures for one text container:

- `CrdtRope` (`tracker/crdt_rope.rs`): a B-tree whose leaves are `FugueSpan`s.
  A single insert of any length is one leaf, and contiguous inserts by one
  peer merge into one leaf.
- `IdToCursor` (`tracker/id_to_cursor.rs`): op id to rope leaf, stored per
  peer as fragments of at most `MAX_FRAGMENT_LEN` (256) ids.

So one inserted text of L characters is one leaf but L/256 fragments.
`Tracker::_checkout` and `Tracker::forward` walk the fragments of the spans
they retreat or forward and emit one `LeafUpdate` per fragment: for a big
insert, L/256 updates that all name the same leaf and carry the same status
change. Two places then did work per update over the whole insert:

1. `CrdtRope::update` passed them to
   `BTree::update_leaves_with_arg_in_ranges`, which split the leaf at every
   fragment boundary (and tested every range against every piece), then put
   the pieces back with `insert_by_path`. Equal status and contiguous ids make
   every piece merge straight back into the first, so the call returned the
   same leaf index once per fragment.
2. `Tracker::update_insert_by_split` took each returned index, read that
   leaf's id span (the whole insert) and rewrote every fragment of it in
   `IdToCursor`.

That is (L/256)² fragment rewrites per checkout across the op. Everything
that checks out the tracker pays it: `checkout`, `diff`, `fork_at`, undo and
redo (two checkouts each), re-attaching, and an import that the diff
calculator has to replay (`import_batch` always, a plain import when there is
concurrency). Measured on 1.17.1 (release build, idle machine): undo of one
1 MB insert 18.7 s, `import_batch` of eight 100 KB updates 5.4 s against
1.1 ms one by one. With the two changes below (loro-dev/loro#1236 and
loro-dev/loro#1082): about 5 ms and 2 ms, and about 25 ms to undo a 5 MB
insert.

## What keeps it linear

- `CrdtRope::update` coalesces adjacent `LeafUpdate`s that name the same
  leaf, have the same `set_future` and `delete_times_diff`, and have
  contiguous forward id spans. The per-fragment updates of one op collapse
  into one, so the leaf is not split at all. It depends on the stable sort by
  leaf and on `IdToCursor::iter` yielding ascending counters; if either
  changed, updates would stop merging, which is slower but still correct.
- `Tracker::update_insert_by_split` updates each reported leaf once, for the
  reports that are not coalesced away.

Merging `[a, b)` and `[b, c)` only removes an interior split point: every id
gets the same `set_future` assignment and the same `delete_times_diff` exactly
once either way. Overlapping spans (two deletes of the same text) are not
contiguous and are never merged. Neither change touches what is encoded or
what a diff contains.

## Tests

- `crates/loro/tests/large_text_time_travel.rs`: random histories whose ops
  span several fragments; checkout, diff, fork and undo/redo against recorded
  states. The fuzz targets insert only short strings, so this is the coverage
  for multi-fragment ops. Dropping a coalesced update makes both tests fail.
- `crates/loro/tests/perf_large_text.rs` (ignored): timings for undo, redo,
  diff, checkout and `import_batch` by text size.
- `crates/loro/tests/perf_giant_op_checkout.rs` and
  `crates/loro/tests/big_insert_import.rs` (ignored timing checks).

## Not covered here

How much of a text's history one undo or checkout replays is a separate
matter (loro-dev/loro#1263); see "Diff modes" in
`crates/loro-internal/docs/diff_calc.md`.
