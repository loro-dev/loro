---
"loro-crdt": patch
---

An import rejected by a memory quota is now rolled back from a log the state records while applying it, instead of recomputing the inverse from history. The rollback always succeeds, even when part of the history cannot be read, and no longer leaves the document at the imported version with an error.
