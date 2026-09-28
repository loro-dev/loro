# Unknown Container Types

Verified against code 2026-09-28.

A container whose type this version doesn't know (`ContainerType::Unknown(k)`,
written by a newer Loro) can be imported, exported, checked out, moved in a
MovableList and deleted. This version never *creates* one: its state is opaque
(`state/unknown_state.rs` decodes to `Null`), so a copy or a recreation would
silently lose content.

## What rejects creation

All of these return `LoroError::ArgErr` built by
`handler::unknown_container_creation_err` before any op is written:

- `insert_container` / `set_container` / `push_container` with an unknown
  container, or with a detached or attached container holding one (copying an
  attached container copies its children):
  `handler.rs` `ensure_no_unknown_container_in_attached`.
- `apply_diff`, `revert_to` and undo/redo when the diff would recreate one:
  `LoroDoc::check_apply_diff_creates_no_unknown_container` (`loro.rs`), run at
  the start of `_apply_diff` and of `undo_internal`.
- `Handler::new_unattached` returns the same error as a backstop.

## Why `_apply_diff` needs a predicting pre-check

Local ops have no rollback, and `_apply_diff` keeps applying the rest of a
batch after an error. So an unknown container has to be found before the first
diff is applied. The apply loop changes its own inputs while it runs, and the
pre-check predicts that from the whole batch:

- *fresh*: container values of Map/List diffs, and the metas of recreated tree
  nodes, are recreated under a new id and recorded in `container_remap`. Diffs
  targeting them then apply to a new, empty container and are never skipped as
  unreachable.
- *revived*: writing a mergeable child's marker makes it reachable again.
- *removed*: deleting a child container (map key overwrite/delete, list delete
  not re-inserted as a MovableList move, tree node delete) makes it and its
  descendants unreachable for the later diffs of the batch, which the loop then
  skips.

Every prediction errs towards checking a diff (a false rejection), never towards
skipping it. Values that are moves in an existing MovableList are decided by
`MovableListHandler::unknown_container_created_by_delta`
(`handler/movable_list_apply_delta.rs`), which reuses `plan_moves` so the move
rules can't drift apart. A diff targeting an unknown container is a no-op in
`Handler::apply_diff` and is not checked.

`crates/loro/tests/unknown_container_atomicity.rs` has the reproductions and a
200-seed random check that every `apply_diff(diff(latest, v))`, `revert_to` and
undo/redo either succeeds or fails without changing the doc.

## Undo

An undo/redo step that would recreate an unknown container is rejected as a
whole: `undo()` returns that `ArgErr`, the doc is unchanged, and the step is
dropped (`UndoManager::perform` in `undo.rs`). The next call undoes the step
before it. The other edits of a rejected step can't be undone any more.
Applying the rest of the step instead would mean dropping the unknown inserts
from the diff, and a wrongly predicted MovableList move would then delete an
existing unknown element for good.

## Events (loro-crdt)

`diff_event_to_js_value` (`crates/loro-wasm/src/lib.rs`) leaves out the event of
an unknown container, and of a container whose diff holds an unknown child that
can't be turned into a JS value (logged with `console.error`). The other events
of the batch are still delivered.

## Mergeable full-state alignment (#1134)

If `apply_diff` gets an opt-in mode that aligns a revived mergeable child's
full-state diff with its current content before applying it, the pre-check has
to look at the aligned diff (or skip those targets). Otherwise it rejects
re-applying a full state that already holds an unknown element.

## Forging unknown containers in tests

- With ops: create a `LoroCounter`, export JSON updates, rewrite `:Counter` to
  `:Unknown(9)` and the counter op content to `FutureOp::Unknown`. Edit the
  typed `JsonSchema` in memory when mergeable markers matter: a string round
  trip turns their `Binary` value into a list.
- As a parent in a snapshot: take a shallow snapshot at the latest version and
  rewrite the child's KV header parent. The checksum is
  `xxh32(bytes[20..], u32::from_le_bytes(*b"LORO"))` in `bytes[16..20]`. See
  `crates/loro-internal/tests/unknown_parent.rs`.
