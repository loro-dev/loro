# Undo application failures

Verified against code 2026-10-13.

`UndoManager::undo_one` and `redo_one` (and the legacy methods) return errors
from inverse application. A failed application leaves the committed document,
its version, the popped item, the opposite stack, cursor metadata and container
remapping as they were before the inverse was applied. No failed operations are
committed or emitted. As before, undo first checkpoints any pending user edits.

On 1.16 this carries the rollback machinery from upstream commit `ccdcee35`
(#1154), restricted to undo: public `apply_diff` and `revert_to` retain their
existing implementation. On main undo uses the existing transaction scope.

The transaction keeps its pre-batch operations and event hints. On failure its
operations are temporarily inserted into the op log under an import rollback
scope. A fresh diff calculator checks state back to the pre-batch version with
event recording suspended, and the temporary history is discarded. Registrations
remain valid, newly created state is removed, exact counter values are restored,
and parent links of discarded operations are forgotten. This avoids an expensive
full-document snapshot on every successful undo and avoids duplicating every
handler's validation logic. Rollback work is proportional to the failed batch.

The transaction mutex also shares a reentrant writer gate. Ordinary transaction
lock acquisitions hold that gate; undo holds it across checkpointing, inverse
calculation, application, commit and stack bookkeeping. A competing writer
cannot commit or contribute operations during the rollback scope. Oplog and
state locks retain their existing lock order. Read-only state queries still use
their existing locks and may observe an intermediate state during an operation;
this is writer serialization, not snapshot isolation for concurrent readers.

History transformation is deferred until application succeeds. An error restores
the popped item to its original remote-diff row rather than using normal push,
which could assign a new transform base. Container remapping is also restored.

On main, the prechecked unknown-container rejection also preserves both stacks
for bounded calls. Legacy undo/redo retain their existing policy of consuming
that rejected item and rebasing older steps over its changes. The bounded
contract is tested for both undo and redo in `unknown_container_atomicity.rs`.

Tests: `crates/loro/tests/undo_apply_atomicity.rs` retains the natural 1.16
mergeable-revival failure (on main that trigger is already repaired).
`crates/loro-internal/src/tests/undo_atomicity.rs` injects an error after real
handler operations to prove single-text and mixed-container rollback, undo/redo,
repeated failure, event silence, version preservation and concurrent writer
serialization. The injection is thread-local and compiled only for unit tests.
