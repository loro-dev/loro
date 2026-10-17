# Undo application failures

Verified against code 2026-10-17 (including #1202 and #1209).

`UndoManager::undo` and `redo` process at most one recorded item. An application
error returns `Err` after rolling back the inverse: document values, text styles,
frontiers, VV, committed operations, both stacks, cursor metadata, container
remapping and MovableList element remapping remain as before application. No
failed operations are committed or emitted. Pending user edits are checkpointed
before the attempt, as before. There are no separate bounded APIs (#1184).

## Rollback and writer serialization

`LoroDoc::undo_internal_with` marks the rollback scope on the existing
transaction while it still holds #1209's preparation guard. Native plans run
through `Plan::apply_with_txn` without releasing that guard. Only after all
native plans finish is the guard released for ordinary handlers, which reuse
the same transaction and rollback scope. `finish_undo_rollback` then completes
or rolls back both phases together. It reuses `begin_apply_diff_scope_with_txn`,
`end_apply_diff_scope`, and `rollback_apply_diff_scope` from public apply-diff
rollback; see [apply-diff-atomicity.md](apply-diff-atomicity.md).
The transaction's operations are temporarily inserted into the op log, state is
checked back with event recording suspended, and the temporary history is
removed. Arena registrations remain valid, new container state and discarded
parent links are cleared, and exact counter values are restored. Successful
undo does not take a full-document snapshot.

The transaction mutex shares a reentrant writer gate. Normal transaction locks
hold it; undo holds it across checkpointing, calculation, application, commit
and history bookkeeping. A competing transaction writer cannot commit or add
operations during rollback. Existing oplog/state lock order remains unchanged.
Read-only queries may still observe intermediate state; this is writer
serialization, not snapshot isolation for concurrent readers.

Undo must own an intact rollback scope. Losing it or failing rollback panics:
returning an ordinary error with partly changed state would make retry unsafe.
Native MovableList application errors remain `LoroError`s and enter this same
rollback path; the port does not restore the old plan `expect`s.

History transformation runs only after successful application. An error restores
the item to its original remote-diff row, keeping its transform base and metadata.
Both container and element identity maps are restored, so ids of discarded
operations cannot leak into a later retry.

## Unknown-container rejection

Keep the existing [unknown-container contract](unknown-containers.md): a
prechecked step that would recreate an unknown container returns `ArgErr`, leaves
the document unchanged, consumes that one step, and rebases older history over
its retained edits. It does not try the next step in the same call. Only the
explicit `rejected_step` marker selects this policy; an `ArgErr` during actual
application follows normal rollback and retry, just like `OutOfBound`.

## Regressions

- `src/tests/undo_atomicity.rs`: errors after real text and mixed-container
  edits, undo and redo, repeated failures, values/deltas/frontiers/VV/operation
  counts/stacks/event silence, retry, and a competing writer.
- `src/undo/atomicity_tests.rs`: a nonempty remote-diff row, metadata and both
  identity maps survive `OutOfBound` and application-time `ArgErr`, including
  parent recreation and native MovableList removal undo. Faults in both the
  held-transaction native phase and the later ordinary phase roll back real
  native edits; retry succeeds.
- `crates/loro/tests/unknown_container_atomicity.rs`: prechecked rejection
  consumes one item, keeps the opposite stack, and leaves older history usable.
- `crates/loro/tests/undo_apply_atomicity.rs`: the original 1.16 mergeable-child
  trigger, already repaired on current main, remains a successful undo.

The error-handling port comes from maghamalyan's #1190, without its #1184 parent.
