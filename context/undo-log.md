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
   empty a `Create` at its previous place.
2. On commit, `build_local_inverse` composes them, newest op first, into a
   `crate::undo::LocalInverse { span, deps, diff }` attached to the commit's
   `InternalDocDiff`/`DocDiff` (`local_inverse`). Events merging several commits
   chain them only when each continues right after the previous
   (`LocalInverse::then`).
3. `UndoManagerInner::record_checkpoint_at` stores it on the stack item when
   its span is exactly the item's; merging an item keeps it only for a
   contiguous run. An undo/redo's own commit is captured while
   `processing_undo` (`processing_inverses`) for the opposite stack's item.
4. `undo_internal_with` uses the stored inverse instead of the two checkouts
   when the item is one run (`split_span_based_on_deps` gives one span), its
   span and deps match, and no movable list is involved. The rest (the
   transform against the stack's composed remote diff, applying it as new ops)
   is unchanged. No checkout happens, so none is undone afterwards.

The inverse must equal what the checkout reports. Rules found by that check:

- Containers created inside the span do not exist at its deps; their entries
  are dropped (`drop_containers_created_in`).
- A checkout reports a map key only when its value changes; entries that
  restore the value the key has after the span are dropped
  (`prune_unchanged_map_entries`, using `LocalInverse::map_values_after`).
  Unset entries (the key did not exist) always stay.
- Deltas are normalized (`normalize_deltas`): adjacent items merged and
  same-attribute text inserts joined.

## Cost

`crates/loro/examples/undo_cold_bench.rs` (first undo after opening from a
snapshot): 1M-char text 0.29 -> 0.06 ms and +1.2 MB -> +221 bytes resident;
10k child maps 6.5 -> 0.04 ms and +5.5 MB -> +484 bytes; 10k-node tree
10.6 -> 0.06 ms and +5.6 MB -> +451 bytes; 40k-node tree 70-81 -> 0.10 ms and
+22.7 MB -> +451 bytes. Movable lists are unchanged (not recorded yet). Recording while an undo manager is
attached (`undo_edit_overhead_bench`, one commit per op): typing about
+0.4 us/op, deleting about +1 us/op (reading the deleted text and styles), map
writes about the same. Nothing is recorded without an undo manager.

## Not recorded yet (undo checks out, as before)

Text marks, movable lists, tree deletions of a subtree or of a node with
metadata, moves out of a deleted parent, writes that would revive a child
container (restoring a deleted container value), map binary values (mergeable markers),
rolled-back transaction batches, and items whose ops are interleaved with other
peers' ops (more than one span).

## Verification

`crate::undo::verify_recorded_inverses()` is on in this crate's tests and in debug
builds with `LORO_VERIFY_UNDO_LOG` set. Then every undo that uses a recorded
inverse also checks out and asserts both diffs mean the same changes
(`assert_same_undo_diff`: checkouts split inserted text at chunk boundaries and
may keep a delete apart from the insert after it, which apply and transform the
same). Run the undo suites in `crates/loro/tests`, `crates/loro-internal/tests/undo.rs`
and `crates/fuzz/tests/undo.rs` with it set after changing any of the above.
