---
"loro-crdt": patch
---

Fix `UndoManager` failing to undo a `MovableList` deletion or anchoring a move/redo to the wrong item after a concurrent remote move revives an element previously restored by undo (#1234).
