---
"loro-crdt": patch
---

Compact internal `Op`, `ImVersionVector`, sparse paged `InnerStore`, and `ListState` child-index layouts, deduplicate identical queued spans in `latest_single_head_critical_version`, shrink spare `StrArena` capacity during full reclamation, and add native out-of-core `FileKvStore` spilling (`MemoryBudget::spill_dir`).
