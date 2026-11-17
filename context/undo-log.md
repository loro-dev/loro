# Undo Without Checkouts: Recorded Inverses

Verified against code 2026-11-17.

## Why

`UndoManager` used to compute each step's inverse by checking the whole
document out at the step's last op and then at its deps
(`LoroDoc::undo_internal_with`, the `calc_diff` closure). A user who never
checks out has no history caches, parsed history or diff trackers; the first
undo built them and they stayed resident. Measured on documents opened from a
snapshot (`crates/loro/examples/undo_cold_bench.rs`, before this change):

| Document | First undo | Second undo | Resident memory added |
| --- | --- | --- | --- |
| 1M-char text | 0.28 ms | 0.06 ms | +1.2 MB (diff calculator) |
| 10k child maps | 6.5 ms | 0.04 ms | +5.5 MB (history cache, parsed history) |
| 10k-node tree | 11 ms | 4.8 ms | +5.6 MB |
| 40k-node tree | 81 ms | 62 ms | +22.7 MB (the document doubled) |

## How

1. `Transaction::apply_local_op` (`txn.rs`) records, for each op, the
   event-level diff that undoes it, read from the state before the op is
   applied (`local_op_inverse`), but only while an undo manager is attached
   (`LocalInverseRecorder`). Text deletes read the deleted text and its styles
   (`slice_delta` by entity range); list deletes the values; map writes the
   previous `MapValue`; counters negate. Trees: a new node's creation becomes a
   `Delete` at its parent and index; a move between live parents a `Move` back
   with the node's previous fractional index; deleting a leaf whose metadata is
   empty a `Create` at its previous place. Movable lists: see below.
2. On commit, `build_local_inverse` composes them, newest op first, into a
   `crate::undo::CommitInverse`, wrapped in a `LocalInverse { span, deps,
   commits }` attached to the commit's `InternalDocDiff`/`DocDiff`
   (`local_inverse`).
3. Events merging several commits, merged undo items, and an undo/redo's own
   commits (captured while `processing_undo` in `processing_inverses` for the
   opposite stack's item) chain them with `LocalInverse::chain`, only when each
   continues right after the previous. Chaining appends the shared commits
   (`Arc::make_mut`, usually in place), so an item merged from n commits costs
   O(n), not O(n²). `UndoManagerInner::record_checkpoint_at` stores it on the
   stack item when its span is exactly the item's.
4. `undo_internal_with` seals it (`LocalInverse::seal`: compose the commits,
   newest first, and apply the rules below once to the whole span) and uses
   the result instead of the two checkouts when the item is one run
   (`split_span_based_on_deps` gives one span) and its span and deps match.
   The movable-list scopes then come from the records
   (`movable_list::scopes_from_records`) instead of reading the deleted
   elements' history (`movable_list::scopes`). The rest (the transform
   against the stack's composed remote diff, applying it as new ops) is
   unchanged. No checkout happens, so none is undone afterwards.

The inverse must equal what the checkout reports. Rules found by that check:

- Containers created inside the span do not exist at its deps; their entries
  are dropped (`drop_containers_created_in`, `created_in`).
- A checkout reports a map key only when its value changes; entries that
  restore the value the key has after the span are dropped
  (`prune_unchanged_map_entries`, using `LocalInverse::map_values_after`).
  Unset entries (the key did not exist) always stay.
- Deltas are normalized (`normalize_deltas`): adjacent items merged and
  same-attribute text inserts joined.
- Counter increments are kept as they are (`CommitInverse::counter_ops`) and
  summed when sealed as the checkout's counter calculator does: from 0,
  subtracting each in op order (`counter_inverses`). Float addition is not
  associative: composing per-op negations newest first turned
  `[1e16, -1e16, 1]` into an undo that changed nothing.

## Movable lists

Movable-list undo keeps its identity plan (`undo/movable_list.rs`,
[movable-list-apply-diff.md](movable-list-apply-diff.md)): removed elements are
recreated with fresh ids (restoring the original id would need a `Move` whose
`from` names a live item, which a step after the deletion cannot have; the
tracker would delete another element's item). What changed is where the plan's
inputs come from. `RecordedInverse` (`undo/movable_list.rs`) reads, while the op
is applied, the element snapshots `Snapshot::capture_span` took by checkout
before and after the span, and the first `RECORDED_CANDIDATES` (2) anchor
candidates on each side (`MovableListState::walk_user`: one query, then
neighbouring leaves). `complete_anchors` still extends a search through history
when none of them survives. `RecordedInverse::to_inverse` gives the same
`Inverse` the checkout produced; `positional_diff` builds the positional list
diff only when the step is undone.

Each record keeps one pair of anchor searches (`sides`; the targets are one
contiguous range), expanded to one pair per target in `to_inverse`, as the
checkout's capture has them.

Recorded: one insert, delete (at most `MAX_RECORDED_DELETE` = 64 elements),
move or set per list per step; several inserts; one deletion the list split into
several ops (dead items between), joined into the original range
(`merge_deletes`). A list created by the span itself is marked not alive before
it, as the checkout saw it. Other combinations, and deleting or setting a child
container, still check out.

One intended difference: on a shallow document a checkout rebuilds an element's
value id from the retained history, and a set trimmed before the shallow root
leaves it with another op's id; the identity plan then issued a set that changed
only the value id. The recorded snapshot reads the live state, so that set is
gone. The verification skips value ids on shallow documents
(`assert_same_inverses`).

## Cost

`crates/loro/examples/undo_cold_bench.rs` (first undo after opening from a
snapshot): 1M-char text 0.29 -> 0.06 ms and +1.2 MB -> +221 bytes resident;
10k child maps 6.5 -> 0.04 ms and +5.5 MB -> +484 bytes; 10k-node tree
10.6 -> 0.06 ms and +5.6 MB -> +451 bytes; 40k-node tree 70-81 -> 0.10 ms and
+22.7 MB -> +451 bytes; 100k-element movable list 0.30-0.33 -> 0.09 ms and
+2.4 MB -> +432 bytes. Recording while an undo manager is
attached (`undo_edit_overhead_bench`, one commit per op): typing about
+0.4 us/op, deleting about +1 us/op (reading the deleted text and styles), map
writes about the same, movable-list moves and deletes about +1.2 us/op.
Nothing is recorded without an undo manager.

What a record keeps counts: an item's inverse, the values it restores and the
map values it compares against included, is part of `undo_bytes` (so
`max_undo_bytes` evicts it), and the inverses of an open transaction are part of
`uncommitted_txn_bytes` (`crates/loro/tests/undo_recorded_inverses.rs`).

## Not recorded yet (undo checks out, as before)

Text marks, movable-list steps outside the shapes above, tree deletions of a subtree or of a node with
metadata, moves out of a deleted parent, writes that would revive a child
container (restoring a deleted container value), map binary values (mergeable markers),
anything touching a mergeable container,
rolled-back transaction batches, a movable list edited by two commits of one
merged item, and items whose ops are interleaved with other
peers' ops (more than one span).

## Verification

`crate::undo::verify_recorded_inverses()` is on in this crate's tests and in debug
builds with `LORO_VERIFY_UNDO_LOG` set. Then the movable-list scopes are read from
history and must match the records (`records_match_scopes`; when they do not,
as for deletes trimmed by a shallow root, the step checks out), and every undo
that uses a recorded inverse also checks out and asserts both diffs mean the
same changes
(`assert_same_undo_diff`: checkouts split inserted text at chunk boundaries and
may keep a delete apart from the insert after it, which apply and transform the
same). Run the undo suites in `crates/loro/tests`, `crates/loro-internal/tests/undo.rs`
and `crates/fuzz/tests/undo.rs` with it set after changing any of the above.
