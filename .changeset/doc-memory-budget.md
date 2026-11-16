---
"loro-crdt": minor
---

Add bounded `LoroDoc` memory budget controls (`MemoryBudget`, `DocMemoryStats`, `setMemoryBudget`, `getMemoryBudget`, `memoryStats`, `reclaimMemory`, `tryReclaimToBudget`, `tryCommit`) with Stage 1/2 watermark hysteresis, cold container state demotion, bounded KV block & decoded value caches, `UndoManager` byte quotas (`maxUndoBytes`, `setMaxUndoBytes`, `usedBytes`), and structured `MemoryLimitExceededError` reporting for oversized imports, pending pools, and budget-exceeding commits.
