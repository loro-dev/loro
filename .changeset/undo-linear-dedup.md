---
"loro-crdt": patch
---

Speed up undo bookkeeping that grew quadratically: rebasing undo items over remote tree moves, and committing one transaction that edits many movable lists while an UndoManager is attached.
