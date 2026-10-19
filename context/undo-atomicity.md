# Undo application failures

Verified against code 2026-10-18 (including #1202 and #1209).

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

`LoroMutex::lock_exclusive` (`src/lock.rs`, `TxnGate`) keeps other threads'
transaction writers out while undo runs checkpointing, calculation, application,
commit and history bookkeeping. Ordinary transaction locks only read the gate's
`active` flag before and after taking the mutex (two atomic loads); they do not
take another lock, so local edits cost the same as without the gate. Undo sets
the flag before it first takes the transaction lock: every other writer either
finishes before that point or sees the flag after acquiring the mutex, then
releases it and waits for the gate. The owning thread, including nested
`lock_exclusive` calls, uses the transaction lock normally. A competing writer
cannot commit or add operations during rollback, and existing oplog/state lock
order is unchanged. The gate ignores mutex poisoning: its owner record is reset
by the guard during unwinding, so a panic inside one undo does not block later
writers or undos. Read-only queries may still observe intermediate state; this
is writer serialization, not snapshot isolation for concurrent readers.
Tests: `lock::txn_gate_tests` (nesting, waiting writers, panic recovery).

Undo must own an intact rollback scope. Losing it or failing rollback panics:
returning an ordinary error with partly changed state would make retry unsafe.
Native MovableList application errors remain `LoroError`s and enter this same
rollback path; the port does not restore the old plan `expect`s.

History transformation runs only after successful application. An error restores
the item to its original remote-diff row, keeping its transform base and metadata.
Both container and element identity maps are restored, so ids of discarded
operations cannot leak into a later retry.

## Deleted remap targets are skipped, not retried

Because a failed step is retried, an error that can never succeed would block that
manager's older history forever. `apply_diff_with_movable_undo` (`src/loro.rs`)
therefore skips a container whose remapped target is already deleted, exactly as
it skips other unreachable containers. A remap target is normally recreated by the
same batch, so remapped containers used to bypass the reachability check. But an
earlier undo may have recreated the container, and a later local or remote edit may
have deleted it again; writing to it then always fails with `ContainerDeleted`
(the transaction's `is_deleted` check). Main swallowed that error and continued,
returning `Ok(false)` when nothing else changed; skipping gives the same result.
Regression: seed 740 in `crates/loro/tests/movable_list_undo_random.rs`.

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
