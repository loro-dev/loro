---
"loro-crdt": patch
---

UndoManager.groupStart now always starts a new undo item. With a merge interval set, the group's first change used to merge into the item before it, so one undo removed both.
