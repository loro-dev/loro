---
"loro-crdt": patch
---

Add Rust-only `UndoManager::undo_one()` and `redo_one()` methods that consume at most one history item, including a no-op. Existing `undo()` and `redo()` retain their no-op-skipping behavior. The new methods are not exposed in the JavaScript/WASM API.
