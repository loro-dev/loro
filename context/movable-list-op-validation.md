# Movable List `Move`/`Set` Validation on Import

Verified against code 2026-09-28 (merged with main `6e294c87`). Text bounds and the rollback-scope list verified 2026-09-30.

Imported movable-list `Move { from, to, elem_id }` and `Set { elem_id, value }` ops
come from other peers, so they are external input. Several shapes of them used
to panic while the doc held its locks (`pos.unwrap()`/`value_id.unwrap()` in
`MovableListState::apply_diff_and_convert`, `idlp_to_id(..).unwrap()` in
`MovableListHistoryCache::last_pos`, `last_value(..).unwrap()` in
`MovableListDiffCalculator::calculate_diff`, `convert_index(..).unwrap()` for
an overrunning list delta, and the diff tracker's B-tree for huge positions).
The panic poisoned a `LoroMutex`, and the process aborted when a destructor hit
the poisoned lock during unwind.

Tests:
- `crates/loro/tests/movable_list_invalid_ops.rs`.
- The binary and `import_batch` cases in `crates/loro-internal/src/tests/import_atomicity.rs`.
- The `ChangeStore` rollback and KV lookup tests in `src/oplog/change_store.rs`.
- `crates/loro-wasm/tests/movable_list_invalid_ops.test.ts`.

## Rejected with `Err`: ops with no meaning

1. **Positions no real sequence can reach.** `InnerListOp::check_positions`
   (`src/container/list/list_op.rs`) runs when binary (`outdated_encode_reordered::decode_op`)
   and JSON (`json_schema::decode_op`) ops are decoded.
   - It rejects any position of a List/MovableList/Text op at or past
     `UNKNOWN_SPAN_LEN - 1` (`src/container/richtext/tracker.rs`). That is the length
     of the tracker's placeholder span for unreplayed history.
   - Positions past it panicked inside the tracker before `validate_diff` could run.
   - This check needs no history, so it covers every import path, including
     detached imports.
   - **It is a hard document limit, not only an import check.** `decode_op` is also
     how a doc parses its own stored change blocks and snapshot blocks
     (`block_encode.rs`), so a sequence position ≥ 1,073,741,822 is rejected there
     too. That is deliberate:
     - Snapshot and update blocks are external input as well.
     - A sequence that long (≈1.07e9 items, ≥1 GB of text) cannot be diffed or
       imported by any peer anyway: the tracker panics before this check existed.
     - Skipping the check for "own" blocks would need a trust flag threaded through
       block decoding.
2. **Unknown or out-of-history element.** `OpLog::validate_movable_list_elem_refs_in_import_scope`
   (`src/oplog.rs`) checks each `Move`/`Set`. `elem_id` must be an `Insert` op in the
   same container that lies in the op's causal history (see the causal check below).
   It returns `LoroError::DecodeError`.
   - The references are recorded by `OpLog::insert_new_change` into the open
     `ImportRollback`, so they cover:
     - directly imported changes,
     - pending changes the import unlocks,
     - every blob of an `import_batch`.
   - The validator reads the recorded list, so it never re-reads the imported range
     from the change store.
   - `OpLog::resolve_movable_list_elem` finds the element with one
     `ChangeStore::get_change_by_lamport_lte` lookup. That lookup scans both parsed
     and KV-only blocks, so a miss means the lamport is not in the stored history.
   - What an element resolves to (insert op, container) does not depend on the op, so
     each pass caches it per `elem_id`. Moves/sets keep hitting the same elements.
   - The causal check reads the cached start version of the op's DAG node
     (`AppDag::ensure_vv_for`); earlier ops of the same node are by the same peer.
     `AppDag::get_vv` would clone and insert into a version vector per op.
   - Only on a shallow doc does a miss fall back to the shallow-root state
     (`ContainerHistoryCache::shallow_root_has_movable_list_elem`): an op after the
     root can only see pre-root elements that are still alive at the root.
   - Call sites:
     - the attached branch of `import_changes_and_apply_delta_to_state_if_needed`
       (when `rollback_enabled`);
     - its detached branch (it opens its own scope when the preflight asks for one
       and no batch scope is open);
     - `BatchImportGuard::finish`, which validates the whole batch once before the
       closing checkout and rolls the whole batch back on failure
       ([import-batch-atomicity.md](import-batch-atomicity.md)).
3. **Out-of-bounds `from`/`to` that survive the import.** `MovableListState::validate_diff`
   bounds-checks the list delta in op-index space (dead list items count), the same
   way `ListState::validate_diff` does.
   - This check needs state, so a detached import cannot run it (see the gaps below).
   - It checks the composed delta, so an out-of-bounds move that a later op in the
     same import cancels (e.g. `move to: 9` followed by `delete pos: 9`) is accepted.
     Every path gives the same result for it.
   - Text has the same check: `RichtextState::validate_diff` bounds-checks the text
     delta in entity-index space (style anchors count). An insert, delete or mark
     past the end of the text used to panic in `insert_elem_at_entity_index` with
     the locks held (loro-dev/loro#1160).

### Why every such import gets a rollback scope

`ImportChangesPreflight` (`OpLog::preflight_import_changes`) and
`PendingChanges::has_state_apply_rollback_ops` set `needs_state_apply_rollback`
for List, MovableList, Text and Tree ops (`oplog::state_apply_can_reject`). An
import without that scope panics if `validate_diff` rejects its diff, so a
container type whose `validate_diff` can fail must be listed there.
- The scope costs a few small clones and journal entries per import: about
  +0.3 µs per single-op Text import (loro-dev/loro#1160 measured 20k imports at
  ~44 → ~50 ms). Large imports are not measurably slower.
- The preflight inspects the ops of **every** new change, including ones whose
  deps are not in the DAG yet. Those deps may be earlier changes of the same import,
  which then unlock them during the import.
- It used to skip such changes before looking at their ops. That let
  `[C1: map-only change, C2: forged op depending on C1]` skip both the rollback
  scope and the validation. On that path a cross-container move was accepted
  silently and put one element in two lists.

## Applied with CRDT semantics: move/set of a deleted element

A `Move`/`Set` that is causally after the delete of its element cannot come from
the public API. It is still **accepted**, with the same meaning as a *concurrent*
move/set of a deleted element. The move creates a new position, so the element
comes back with its last value. The set changes a value nobody can see. Why:

- It gives a forger nothing new. The forger can declare deps from before the
  delete and get the same effect from a legitimate concurrent move.
- Rejecting it consistently is not affordable. Checkout-mode replay (`checkout`,
  `import_batch`'s closing reattach, concurrent imports, detached → `attach`) would
  have to know whether the element is alive at the op's version. That needs an
  index from list items to the deletes that cover them. If only the linear path
  rejected it, `import` and `import_batch` of the same bytes would disagree.

The one path that could not apply it is the forward fast path
(`DiffMode::Linear`/`ImportGreaterUpdates`). There the movable-list diff only
carries the fields the op touched, and `apply_diff_and_convert` fills in the rest
from the element in `DocState`. Detection and fallback:
- `MovableListState::references_absent_elem` / `DocState::needs_checkout_diff`
  detect a delta that moves/sets an element missing from the state.
- `recalc_in_checkout_mode_if_needed` (`src/loro.rs`) recomputes that import's
  diff with `DiffCalculator::new(true)` (Persist, so always Checkout mode). This
  matches a full replay.
- Honest imports never trigger it.
- `validate_diff` still returns `Err` for such a delta as a backstop.

## Change-store pitfalls this work exposed

- **`decode_block_range` read a version varint that blocks do not have.**
  `encode_block` writes a postcard `EncodedBlock` with no version prefix, so every
  field was read one position off.
  - History: `3d2d9d9c` (2024-09, "refactor: optimize block encoder") removed the
    `version` field from the block, its encoder and the full decoder, but not from
    `decode_block_range`.
  - Every `loro-crdt@1.0.0*` release contains that commit, so no 1.x build ever wrote
    version-prefixed blocks and the fix cannot make stored data unreadable.

  Consequences:
  - Blocks that do not start at counter 0 were skipped.
  - Block `0@P` got its lamport length as its lamport start.
  - Lamport lookups on KV-only blocks were wrong:
    `get_change_with_lamport_lte` / JS `getChangeAtLamport` after a snapshot load
    (this was also broken on main), and the element validator above.
  - Existing tests missed it because they parsed every block first.
- **Rollback must not leave an old block in front of the next insert.**
  `insert_change_inner` merges a change into the cached block right before it.
  - Failure sequence: an import creates a peer's newest block without loading the
    older ones; a read during the scope (such as the validator's lamport lookup)
    caches an older KV block; rollback removes the newest block. The next insert of
    the same change then panicked with "counter should be continuous".
  - `ChangeStore::rollback_import` therefore evicts the flushed blocks of every peer
    the rollback touched. They reload from KV on demand.
  - The lookup's parse also registers containers that the arena rollback forgets,
    so rollback then drops the parsed changes of the blocks parsed since the
    import began ([arena-parent-links.md](arena-parent-links.md#import-rollback)).
- **Cheap rollback records.** `ChangeStoreRollback` keeps a `BlockShape` for each
  unflushed pre-scope block an import appended to: change count, the last change's
  op count and its last op. Rollback truncates back to it.
  - Imports only append (push changes, or push ops that may merge into the last op),
    so the shape is enough.
  - Flushed blocks need no record: their KV copy is the pre-scope version.
  - The earlier record was an `Arc` of the whole block, which made the next append
    copy the block's changes on every import under a scope. That made small
    movable-list imports about 40% slower, and List/Tree imports on main already
    paid it (they are about 45% faster now).

## Known gaps (not fixed here)

- A `Move` whose `from` does not point at its element is accepted. The tracker
  removes whatever list item sits at `from`, so another element can disappear.
  The result is the same on every path, but it does not match any honest op.
- An explicitly detached doc that imports an op which only state validation
  rejects (an out-of-bounds list insert, or a movable-list move out of bounds)
  panics on `attach()`/`checkout_to_latest`. Those return `()` and `expect` the
  checkout. This affects every container type and predates this change.
- The oplog inside a `FastSnapshot` is not validated op by op, because that would
  decode every block. A forged snapshot can still reach the diff calculator's
  unwraps on a later checkout.
- A forged change parked as pending fails the import that later unlocks it, so
  that import is rolled back, the same as for list bounds errors.
