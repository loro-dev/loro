---
"loro-crdt": patch
---

Fix UndoManager's merge timer when an import or another history boundary forces a new undo item. Subsequent local edits now use the new item's merge window, while edits that actually merge keep the original window.
