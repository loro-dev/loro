---
"loro-crdt": patch
---

Fix a panic on import that could permanently break a document's sync.

`import_changes_to_oplog` classifies the changes it cannot apply and hands them
to `extend_pending_changes_with_unknown_lamport` to be parked in the pending
store. In between, `try_apply_pending` applies changes that were already parked
from earlier imports, which advances the oplog version. A change that was
un-appliable at classification time can therefore be applicable — or already
applied — by the time it is filed, and both outcomes hit an `unreachable!`.

Reproducing it only needs a `D <- X <- Y` dependency chain: park `X` by
importing it alone, then import one update carrying `D` and `Y` but not `X`.
Applying `D` releases `X` from the store, which makes `Y` applicable and trips
the panic.

The re-classification is now handled instead of asserted: a change that became
applicable is applied (cascading into any changes it unblocks), one that was
already applied is dropped, and `ImportStatus` reports only what genuinely
stayed pending rather than everything the first classification pass rejected.

This mattered most through `import_batch`, which force-detaches the document for
the duration of the batch and reattaches after the loop. The panic unwound past
the reattach and left the document detached forever, so every later import and
export failed and the document stopped syncing until the process restarted.
