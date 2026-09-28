---
"loro-crdt": patch
---

Importing a malformed movable-list `move` or `set` op no longer aborts the process. These ops used to panic inside the document locks, poisoning them, and the process then aborted during unwind. Now:

- A `move`/`set` whose element id is not an element of the same list in the op's causal history, or a `move` whose index is past the end of the list, makes `import`, `importJsonUpdates` and `importBatch` return an error. Nothing from that import is applied and the document stays usable. This also holds when the op depends on another change in the same import, which used to skip validation.
- Insert, delete and move positions past about 2^30 in a List, MovableList or Text op are rejected when the update is decoded, instead of panicking during import.
- A `move`/`set` that targets an element deleted earlier in its own history is applied the same way a concurrent move/set of a deleted element is: the move brings the element back, and the set is not visible. Every import path, and a replay of the full history, gives the same result.

Also fixed:

- `getChangeAtLamport` returned `undefined` or the wrong change for changes that were loaded from a snapshot and not yet read. The change store misread the lamport range of stored blocks.
- A rolled-back import could leave a cached change block behind that made the next import of the same changes panic with "counter should be continuous".
- Imports of List and Tree ops (and now MovableList ops) no longer copy the peer's last change block on every import to keep a rollback copy. Many small List imports are about 45% faster.
