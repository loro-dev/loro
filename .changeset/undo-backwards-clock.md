---
"loro-crdt": patch
---

Prevent automatic undo merging when the wall clock moves backwards, including when `mergeInterval` is zero. Explicit undo groups continue to merge normally.
