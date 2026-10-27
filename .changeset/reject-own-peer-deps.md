---
"loro-crdt": patch
---

Reject imported changes that depend on an op of their own peer other than the previous one. Such a change made the import panic with "lamport is not continuous" and poisoned the document lock. The import now returns a decode error and leaves the document unchanged.
