---
"loro-crdt": patch
---

Fix panic in `UndoManager` when `maxUndoSteps` trimming encounters an empty front stack row left by a prior undo with remote diffs.
