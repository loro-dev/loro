---
"loro-crdt": patch
---

Speed up local text editing (~35% faster on the B4 editing trace). Three hot-path
changes: the lock-order debug instrumentation is now compiled out of release
builds (it ran on every per-op lock acquisition); the visible-op count is bumped
incrementally for local ops instead of recomputing it from the version vectors
(which also allocated) on every op; and a couple of per-op allocations on the
text insert/delete path were removed (lazy error-context formatting and inline
storage for entity ranges).
