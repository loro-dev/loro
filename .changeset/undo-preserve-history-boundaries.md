---
"loro-crdt": patch
---

Fix `UndoManager.undo()` and `redo()` consuming additional history items when the top item becomes a no-op after remote edits. Each call now consumes at most one recorded item, preserving unrelated edits and parent containers for subsequent calls. This also applies to Rust `UndoManager::undo()` and `redo()`.

A consumed no-op returns `false` and creates no opposite-stack item. Check `canUndo()` / `canRedo()` (Rust: `can_undo()` / `can_redo()`) for remaining history instead of treating `false` as an empty stack. Grouping and the merge interval still define one history item.
