---
"loro-crdt": patch
---

Prevent memory-budget reclamation thrashing and reduce flush write amplification:
- Protect active and recently refaulted hot containers during soft watermark demotion, with cooldown refresh under sustained hot traffic and deterministic bypass on `reclaim_memory()` / hard-limit rescue.
- Detect rapid Stage 1 cache-rebuild thrashing (such as repeated `block_cache` clear + `get_deep_value()` rebuild cycles) in `SoftBackoff` while preserving periodic probes and hard-limit rescue.
- Route global import and WASM linear-allocation preflights through `SoftBackoff` (respecting configured watermarks and bypassing backoff whenever `projected > cap`), and run global batch preflight once per `import_batch` instead of per blob.
- Retain the most recently used `DiffCalculator` trackers (LRU) during Stage 1 `reclaim_cold_caches` instead of evicting the largest active trackers first.
- Bypass `SoftBackoff` when rescuing over hard memory limits, compact parsed history blocks during `compact_history()`, avoid starving post-rollback history compaction, skip futile Stage 2 demotion on unrescuable `try_commit` rejections, and clean up transient `DiffCalculator` caches after budget-rejected commits.
- Flush `MemKvStore` / `FileKvStore` (`loro-kv-store`) incrementally to L0 SSTables with size-tiered suffix compaction and in-memory L0 Bloom filters instead of rewriting the full table on every flush (note for Rust `loro-kv-store` consumers: `flush()` may now retain multiple SSTables until `export_all()` or full compaction).
