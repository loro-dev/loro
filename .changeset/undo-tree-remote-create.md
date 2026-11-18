---
"loro-crdt": patch
---

Fix `UndoManager.undo()` failing with "Index out of bound" after importing a remote change that re-creates a local tree node in the import's event (#1310). The node's metadata is re-sent in full in that event, and undo treated it as a remote edit.
