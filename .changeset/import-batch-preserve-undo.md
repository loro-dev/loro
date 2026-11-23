---
"loro-crdt": patch
---

Keep UndoManager undo/redo history across multi-blob `importBatch`. Its events now use `by: "import"` and an empty `origin`, matching standalone imports, instead of `by: "checkout"`.
