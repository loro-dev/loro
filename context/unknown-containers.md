# Unknown Container Types

Verified against code 2026-09-30 (after #1134, #1152, #1153; third review of #1142; #1154).

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
  the start of `_apply_diff` and of `undo_internal_with`.
- `Handler::new_unattached` returns the same error as a backstop.

## Why `_apply_diff` checks before applying

`_apply_diff` keeps applying the rest of a batch after an error. `apply_diff`
and `revert_to` now roll a failed batch back
([apply-diff-atomicity.md](apply-diff-atomicity.md), #1154), but undo/redo
calls `_apply_diff` without a rollback and keeps what applied. So an unknown
container has to be found before the first entry is applied; for `apply_diff`
the check is also the cheaper rejection and keeps its debug assertions on the
loop. The loop changes its own inputs as it runs, so the check predicts that
from the whole batch:

- *fresh*: container values of Map/List diffs, the metas of created or moved
  tree nodes, and the mergeable children of those are recreated under new ids
  (`container_remap`). Their entries apply to new, empty containers and are
  never skipped.
- *revived*: writing a mergeable child's marker makes it and its descendants
  reachable again. The loop writes the marker of `mergeable(parent, key)`,
  with the parent as the loop resolves it, and remaps the value to it. When
  that isn't the value's own id (a parent the undo manager already remapped,
  or a value under another key), the child is checked as new and empty.
- *removed*: deleting a child container (map key overwrite/delete, list delete
  not re-inserted as a MovableList move, tree node delete) makes it and its
  descendants unreachable for the later entries, which the loop then skips.

Ancestors come from arena parents (`LoroDoc::arena_ancestors_and_self`), not
from `get_path_to_container`: a hidden mergeable container has no path, but a
batch that revives it makes its descendants reachable.

Every prediction errs towards checking an entry (a false rejection), never
towards skipping it. Values that are moves in an existing MovableList are
decided by `MovableListHandler::unknown_container_created_by_delta`
(`handler/movable_list_apply_delta.rs`), which reuses `plan_moves`. An entry
targeting an unknown container is a no-op in `Handler::apply_diff` and is not
checked.

The check returns the entries holding unknown containers that it predicted to
be skipped. In debug builds the loop asserts that it skips them, that it never
applies unknown containers from an entry the plan below didn't check as
applied, and that no entry of an accepted batch fails on creating an unknown
container (`handler::is_unknown_container_creation_err`).

## Full-state batches

A batch with `full_state` set (from `LoroDoc::diff`; JS `applyDiff(diff, {
fullState: true })`) is applied with `align_revived_mergeable`: the full state
of a re-activated mergeable child, and of the children that alignment keeps,
becomes an edit of the state this doc kept for it (`handler/full_state.rs`).
Keeping an unknown container there creates nothing, so the raw full state must
not be checked.

When the batch holds an unknown container value, `LoroDoc::plan_full_state_batch`
computes the alignment of the whole batch before anything is applied:
`align_full_state` has no side effects (a counter gets an increment diff). The
check runs on the planned edits, and the loop applies them instead of aligning
again, so alignment runs once. Batches without unknown values skip the plan and
the loop aligns each entry itself, as before. The plan doesn't count the marker
writes of an entry whose target is unreachable before the batch, because the
loop skips it (an entry that revives it would have come first; the batch order
is up to the caller). The loop still
keeps its own `full_state_targets`, because only it follows `container_remap`
as containers are recreated. It uses a planned edit only when it aligns the
container the plan aligned. The other case is a mergeable child of a container
the batch recreates: the plan marks it fresh (checked as new and empty), and
the loop aligns it against the new child.

## Undo

An undo/redo step that would recreate an unknown container is rejected as a
whole: `undo()` returns that `ArgErr`, the doc is unchanged, and the step is
dropped (`UndoManager::perform` in `undo.rs`). The next call undoes the step
before it. The other edits of a rejected step can't be undone any more. With a
merge interval, the step is every edit merged into it (loro-crdt's default is
1 s).

- The step's `before_diff` (which rebases the manager's other steps over this
  one) runs only once the step is accepted.
- The rejected step's own changes stay in the doc, so `undo_internal_with`
  returns them and `perform` composes them into the remote diff of the step's
  row: the steps before it are rebased over them like over remote changes.
  That includes the limits of rebasing: if the rejected step moved an element
  an earlier step inserted, undoing the earlier step finds nothing to do and
  falls through to the step before it, exactly as after the same move by a
  remote peer (`undo_after_a_rejected_move_matches_a_remote_move`).
- Applying the rest of a rejected step would mean dropping the unknown inserts
  from the diff, and a wrongly predicted MovableList move would then delete an
  existing unknown element for good.
- `ArgErr` is the rejection signal: `undo_internal` returns no other `ArgErr`.
  Other `_apply_diff` errors of an accepted step are still only logged.

`crates/loro/tests/unknown_container_twin_oracle.rs` checks undo against a twin
with known containers, and that undoing everything leaves exactly the edits of
the rejected steps.

## Events and `diff()` (loro-crdt)

`diff_event_to_js_value` (`crates/loro-wasm/src/lib.rs`) leaves out the event of
an unknown container. A map event that sets a key to an unknown child leaves out
only that entry (`map_delta_to_js` in `convert.rs`) and keeps the other keys; in
JSON form (`for_json`) the child is written as its container id like any other.
A list or movable-list event that inserts an unknown child is still left out
whole (logged with `console.error`), because dropping one item would shift the
indices of the rest. The other events of the batch are always delivered
(#1142, #1151).

There is no JS value for an unknown container: handlers throw, and `toJSON` shows
it as `null`. Delivering the unknown child itself (the left-out map entry or list
item) needs a public representation, e.g. `null` as in `toJSON`, its
`🦜:cid:...` string, or a wrapper class. That is an API decision left open.

`LoroDoc.diff()` leaves out the entries of unknown containers. Applying one is a
no-op in this version, also in a full-state batch (the final clear of an
unknown container does nothing). A newer version that knows the type reads a
missing full-state entry as "empty", though: applying such a `diff()` there
with `fullState` clears the hidden state of a kept container of that type.

JSON updates can't carry mergeable markers (their `Binary` value becomes a list
of numbers), so loro-crdt tests import docs forged in Rust:
`crates/loro-wasm/tests/fixtures/unknown_mergeable_holders.ts`, regenerated by
the ignored `write_wasm_fixture` test in
`crates/loro/tests/unknown_container_atomicity.rs`.

## Tests and cost

- `crates/loro/tests/unknown_container_atomicity.rs`: reproductions from the
  reviews of #1142.
- `crates/loro/tests/unknown_container_twin_oracle.rs`: random edits on a
  forged doc and on a twin holding Counters. Every `apply_diff` (full state and
  incremental), `revert_to` and undo/redo must be rejected without changes or
  match the twin. One variant adds a remote peer, undo/redo interleaved with
  edits and syncs, reordered batches and 3-level mergeable chains (fork peer
  ids are pinned so failures reproduce). `LORO_UNKNOWN_TWIN_SEEDS` runs more
  than the default 60; run it in release with
  `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true` to keep the loop's asserts.
- `crates/loro/tests/perf_unknown_container_apply_diff.rs`: ignored benchmarks.

## Forging unknown containers in tests

`crates/loro/tests/unknown_container_support/mod.rs`:

- With ops: create a `LoroCounter`, export JSON updates, rewrite `:Counter` to
  `:Unknown(9)` and the counter op content to `FutureOp::Unknown`. Edit the
  typed `JsonSchema` in memory, because a string round trip turns the `Binary`
  mergeable markers into lists.
- As a parent in a snapshot: take a shallow snapshot at the latest version and
  rewrite the child's KV header parent. The checksum is
  `xxh32(bytes[20..], u32::from_le_bytes(*b"LORO"))` in `bytes[16..20]`. See
  `crates/loro-internal/tests/unknown_parent.rs`.
