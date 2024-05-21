---
"loro-wasm": patch
"loro-crdt": patch
---

Refine undo impl

- Add "undo" origin for undo and redo event
- Allow users to skip certain local operations
- Skip undo/redo ops that are not visible to users
- Add returned bool value to indicate whether undo/redo is executed
