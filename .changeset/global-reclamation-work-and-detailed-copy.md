---
"loro-crdt": patch
---

Avoid clearing shared blocks below the reclamation target and repeating Stage 1 in the Stage 2 sweep. Track reads through retained container handles for document LRU. Copy each small importBatchDetailed buffer into WASM once while preserving per-blob admission errors.
