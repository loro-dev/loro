---
"loro-crdt": patch
---

Fix `revertTo` and undo adding a mergeable child's content twice after its map key was deleted. For example, `revertTo` a version before `map.delete("s")` turned a mergeable text `"hello"` into `"hellohello"`, a counter `7` into `14`, and a list `["keep"]` into `["keep", "keep"]`. They now write only the real difference and keep the child's characters, list elements and tree node ids.

`applyDiff` accepts a new option, `applyDiff(diff, { fullState: true })`. Use it when applying the result of `doc.diff()` to a document that may still keep a deleted mergeable child the diff restores, such as the same document or a fork: the child's full content is then aligned with that hidden state instead of appended to it. Without the option, `applyDiff` behaves as before: that is right for documents without such hidden state, and for events forwarded to a document that shares the source's hidden state. Event shapes are unchanged. To build a mirror from scratch, use `diff()` or a snapshot.

`UndoManager` also keeps a peer's own edit undoable after a remote undo restores the child it edited.
