---
"loro-crdt": minor
---

Add process/WASM-instance-wide `setGlobalMemoryBudget`, `getGlobalMemoryBudget`, `globalMemoryStats`, `reclaimGlobalMemory`, and `tryReclaimGlobalToBudget` APIs with cross-document LRU reclamation, a shared global SSTable block cache, WASM allocator live-heap and linear-memory tracking, and pre-WASM-malloc admission (`MemoryLimitKind::GlobalBudget`).
