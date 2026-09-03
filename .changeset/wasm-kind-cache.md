---
"loro-crdt": patch
---

perf: cache `kind()` results on container wrappers. `kind()` returns a constant string per container class; it is now memoized after the first call, so repeated reads (e.g. tree traversal in loro-mirror) no longer cross into WASM or allocate a fresh JS string. This extends the existing `id` cache to `kind()`.
