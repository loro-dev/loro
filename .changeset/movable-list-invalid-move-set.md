---
"loro-crdt": patch
---

Importing a malformed movable-list `move` or `set` op no longer aborts the process. These ops used to panic inside the document locks, poisoning them, and the process then aborted during unwind. Now:

- A `move`/`set` whose element id is not an element of the same list in the op's causal history, or a `move` whose index is past the end of the list, makes `import`, `importJsonUpdates` and `importBatch` return an error. Nothing from that import is applied and the document stays usable.
- A `move`/`set` that targets an element deleted earlier in its own history is applied the same way a concurrent move/set of a deleted element is: the move brings the element back, and the set is not visible. Every import path, and a replay of the full history, gives the same result.
