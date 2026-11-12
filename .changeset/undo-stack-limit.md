---
"loro-crdt": patch
---

Enforce `setMaxUndoSteps` immediately when lowering the limit and when redo pushes an undo step. Oldest undo items are discarded with their metadata; a limit of zero retains no undo steps while preserving redo history.
