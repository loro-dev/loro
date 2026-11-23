---
"loro-crdt": patch
---

Keep UndoManager undo/redo history across multi-blob `importBatch`. Its events change `by` from `"checkout"` to `"import"`, while preserving the existing `origin: "checkout"` contract.
