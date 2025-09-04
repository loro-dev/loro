---
"loro-crdt": minor
---

feat: add APIs to fetch values associated with the top Undo and Redo stack entries (#790)

- JS/WASM: `undo.topUndoValue()` and `undo.topRedoValue()` return the `value` from the top undo/redo item (or `undefined` when empty).
- Rust: `UndoManager::{top_undo_meta, top_redo_meta, top_undo_value, top_redo_value}` to inspect top-of-stack metadata and values.
- Internal: stack now supports peeking the top item metadata without mutation.

This enables attaching human-readable labels via `onPush`/`onPop` and retrieving them to keep Undo/Redo menu items up to date.
