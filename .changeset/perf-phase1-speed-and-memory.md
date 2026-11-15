---
"loro-crdt": minor
---

Improve snapshot/export speed, reduce peak import and export memory, rewrite WASM `toJSON()` to stream directly from shallow state without building an intermediate tree, stream large WASM `importBatch` / `importBatchDetailed` inputs, and expose `freeHistoryCache()`, `freeDiffCalculator()`, and `compactChangeStore()` on `LoroDoc`.
