---
"loro-crdt": patch
---

Importing a malformed movable-list `move` or `set` op no longer aborts the process. These ops used to panic inside the document locks, poisoning them, and the process then aborted during unwind. Now:

- A `move`/`set` whose element id is not an element of the same list in the op's causal history, or a `move` whose index is past the end of the list, makes `import`, `importJsonUpdates` and `importBatch` return an error. Nothing from that import is applied and the document stays usable. This also holds when the op depends on another change in the same import, which used to skip validation.
- Insert, delete and move positions of 1,073,741,822 (about 2^30) or more in a List, MovableList or Text op are rejected when the op is decoded, instead of panicking during import. This is a hard limit on sequence length: it also applies when a document reads its own stored history or a snapshot. No peer could import such a document before either.
- A `move`/`set` that targets an element deleted earlier in its own history is applied the same way a concurrent move/set of a deleted element is: the move brings the element back, and the set is not visible. Every import path, and a replay of the full history, gives the same result.

Also fixed:

- `getChangeAtLamport` returned `undefined` or the wrong change for changes that were loaded from a snapshot and not yet read. The change store misread the lamport range of stored blocks, a leftover from the block format change in 1.0. On a snapshot-loaded peer with many blocks, 42 of 669 sampled lamports returned `undefined` and 625 returned the wrong change; all 669 are now correct.
- A rolled-back import could leave a cached change block behind that made the next import of the same changes panic with "counter should be continuous".
- Imports of List and Tree ops no longer copy the peer's last change block on every import to keep a rollback copy. Many small List or Tree imports are about 30% faster.

Performance: movable-list imports now check every `move`/`set` against the document's history. That makes them about 2–5% slower (many small imports) and 0–4% slower (a 120k-op import, about 70% of it moves and sets).
