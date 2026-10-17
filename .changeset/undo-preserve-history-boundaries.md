---
"loro-crdt": patch
"loro.js": patch
---

Fix `UndoManager.undo()` and `redo()` consuming additional history items when the top item becomes a no-op after remote edits. Each call now consumes at most one recorded item, preserving unrelated edits and parent containers for subsequent calls. This also applies to Rust `UndoManager::undo()` and `redo()`.

A consumed no-op returns `false` and creates no opposite-stack item. Check `canUndo()` / `canRedo()` (Rust: `can_undo()` / `can_redo()`) for remaining history instead of treating `false` as an empty stack. Grouping and the merge interval still define one history item.

Align loro.js `UndoManager.undo()` and `redo()` with this result: consuming a no-op returns `false` rather than reporting a change. The item is still consumed and other history remains available.

Record local commits whose operations cancel in the state diff as undo items in loro.js, so undo consumes that no-op instead of reverting an earlier item. Public state subscriptions remain silent for empty diffs.
