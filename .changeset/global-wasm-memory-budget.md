---
"loro-crdt": minor
---

Add process-wide and WASM-instance-wide memory budgets, statistics and cross-document reclamation through `setGlobalMemoryBudget`, `getGlobalMemoryBudget`, `globalMemoryStats`, `reclaimGlobalMemory` and `tryReclaimGlobalToBudget`. Track WASM live heap and linear memory, report retained pages through `wasmLinearMemoryExceeded`, and check admission before large WASM input allocations. Incremental native accounting and reclamation backoff keep routine commits inexpensive.
