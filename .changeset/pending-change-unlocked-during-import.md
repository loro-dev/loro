---
"loro-crdt": patch
---

Fix a panic on import that could permanently break a document's sync.

`import_changes_to_oplog` defers the changes whose deps are not yet in the DAG
and hands them to `extend_pending_changes_with_unknown_lamport` to be parked.
`try_apply_pending` runs in between and applies changes parked by earlier
imports, which advances the oplog version, so a deferred change can become
applicable — or already applied — by the time it is parked. Both outcomes used
to hit `unreachable!`, compiling to `RuntimeError: unreachable` in WASM.

The re-classification is now handled instead of asserted: a change that became
applicable is applied and cascaded into whatever it unblocks, an already-applied
one is skipped, and `ImportStatus` reports only what genuinely stayed pending.

This mattered most through `import_batch`, which force-detaches the document for
the duration of the batch and reattaches after the loop. The panic unwound past
the reattach and left the document detached, so every later import and export
failed and the document stopped syncing until the process restarted.
