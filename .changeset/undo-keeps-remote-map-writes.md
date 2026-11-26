---
"loro-crdt": patch
---

`UndoManager` undo/redo no longer overwrites a map key (including tree node metadata) or a `LoroMovableList` element that another peer set after the local edit. Before, undoing a local `map.set("k", …)` after importing another peer's later write to `k` deleted or reverted that peer's value.
