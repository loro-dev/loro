---
"loro-crdt": patch
---

Fix `revertTo`, `applyDiff(diff(a, b))` and undo adding a mergeable child's content twice after its map key was deleted. For example, `revertTo` a version before `map.delete("s")` turned a mergeable text `"hello"` into `"hellohello"`, a counter `7` into `14`, and a list `["keep"]` into `["keep", "keep"]`. `revertTo` and undo now write only the real difference and keep the child's characters, list elements and tree node ids. `diff()` and events still report the restored child with its full content, and `applyDiff` now reconciles that content with whatever hidden state the receiving doc has, so it also works on a doc that never saw the child. Local events for a restored mergeable child now carry its full content too, like import and checkout events. `UndoManager` keeps a peer's own edits undoable after a remote undo restores the child.
