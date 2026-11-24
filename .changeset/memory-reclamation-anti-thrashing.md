---
"loro-crdt": patch
---

Prevent memory-budget reclamation thrashing and reduce flush write amplification:
- Protect active and recently refaulted hot containers during soft watermark demotion, with cooldown refresh under sustained hot traffic and deterministic bypass on `reclaim_memory()` / hard-limit rescue.
- Route global import and WASM large-string allocation preflights through `SoftBackoff` (respecting both `maxHeapBytes` and `maxWasmLinearBytes` plus configured watermarks), and run global batch preflight once per `import_batch` instead of per blob.
- Retain the most recently used `DiffCalculator` trackers (LRU) during Stage 1 `reclaim_cold_caches` instead of evicting the largest active trackers first.
- Bypass `SoftBackoff` when rescuing over hard memory limits, compact parsed history blocks during `compact_history()`, avoid starving post-rollback history compaction, and clean up transient `DiffCalculator` caches after budget-rejected commits.
- Flush `MemKvStore` / `FileKvStore` incrementally to L0 SSTables with size-tiered suffix compaction instead of rewriting the full table on every flush.
