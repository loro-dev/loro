# Movable List `Move`/`Set` Validation on Import

Verified against code 2026-09-27.

Imported movable-list `Move { from, to, elem_id }` and `Set { elem_id, value }` ops
come from other peers, so they are external input. Several shapes of them used
to panic while the doc held its locks (`pos.unwrap()`/`value_id.unwrap()` in
`MovableListState::apply_diff_and_convert`, `idlp_to_id(..).unwrap()` in
`MovableListHistoryCache::last_pos`, `last_value(..).unwrap()` in
`MovableListDiffCalculator::calculate_diff`, `convert_index(..).unwrap()` for
an overrunning list delta). The panic poisoned a `LoroMutex`, and the process
aborted when a destructor hit the poisoned lock during unwind.

Tests: `crates/loro/tests/movable_list_invalid_ops.rs`, plus the binary and
`import_batch` cases in `crates/loro-internal/src/tests/import_atomicity.rs`.

## Rejected with `Err`: ops with no meaning

1. **Unknown or out-of-history element.** `OpLog::validate_movable_list_elem_refs_since`
   (`src/oplog.rs`) checks every newly imported `Move`/`Set`. `elem_id` must
   resolve through `idlp_to_id` to an `Insert` op in the same container, and that
   op must be in the op's causal history (`AppDag::get_vv`). The lamport shortcut
   `elem_id.lamport >= op lamport` rejects early. It returns `LoroError::DecodeError`.
   - Before a shallow root, history is trimmed. There the element must exist in the
     shallow-root state (`ContainerHistoryCache::shallow_root_has_movable_list_elem`).
     Any other pre-root element was deleted before the root, and no op after the
     root can see it. Import already rejects changes whose deps reach below the root.
     If this check is removed, `last_pos` panics during a Checkout-mode diff.
   - It runs after the changes are in the `OpLog` and before any diff is
     calculated, inside an import rollback scope. It needs the op's causal history,
     which is only known once pending changes are unlocked and lamports are
     assigned. `insert_new_change` cannot return `Err`.
   - Call sites: the attached branch of
     `import_changes_and_apply_delta_to_state_if_needed`, only when
     `rollback_enabled`, which the preflight sets whenever the imported or
     unlocked pending changes hold movable-list ops; its detached branch;
     `update_oplog_and_apply_delta_to_state_if_needed` (legacy encodings), and
     `BatchImportGuard::finish`. An attached `import_batch` owns one rollback
     scope for the whole batch, so per-blob validation is skipped and
     `validate_movable_list_elem_refs_in_import_scope` checks the whole batch once
     before the closing checkout. On failure the whole batch is rolled back
     ([import-batch-atomicity.md](import-batch-atomicity.md)).
   - A detached import (not inside a batch) now opens its own rollback scope when
     `preflight.needs_state_apply_rollback`, so it can reject the blob.
2. **Out-of-bounds `from`/`to`.** `MovableListState::validate_diff` bounds-checks the
   list delta in op-index space (dead list items count), the same way
   `ListState::validate_diff` does. `ImportChangesPreflight` and
   `PendingChanges::has_state_apply_rollback_ops` now include `MovableList`.
   Without that, a release build has no rollback scope and hits the
   "state apply returned Err ... without rollback guard" panic.

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
  resolves elements from the history index and matches a full replay.
- Honest imports never trigger it, so the fast path costs one hash lookup per
  element that has an incomplete delta.
- `validate_diff` still returns `Err` for such a delta as a backstop, so a new
  caller that skips the fallback fails loudly instead of panicking.

## Known gaps (not fixed here)

- A `Move` whose `from` does not point at its element is accepted. The tracker
  removes whatever list item sits at `from`, so another element can disappear.
  The result is the same on every path, but it does not match any honest op.
- An explicitly detached doc that imports an op which only state validation
  rejects (a list insert out of bounds, a movable-list move out of bounds) panics
  on `attach()`/`checkout_to_latest`. Those return `()` and `expect` the checkout.
  This affects every container type and predates this change.
- The oplog inside a `FastSnapshot` is not validated op by op, because that would
  decode every block. A forged snapshot can still reach the diff calculator's
  unwraps on a later checkout.
- A forged change parked as pending fails the import that later unlocks it, so
  that import is rolled back, the same as for list bounds errors.
