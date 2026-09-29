---
"loro-crdt": patch
---

`applyDiff` on a `LoroMovableList` now keeps every child container that the diff moves. Before, a diff that moved two or more children at once, such as `[{ delete: 2 }, { insert: ["🦜:" + b.id, "🦜:" + a.id] }]` to swap two `LoroText` children, could drop one of them or put elements in the wrong order. `UndoManager.undo()` and `redo()` go through the same code, and after some movable-list edits they could panic with an `OutOfBound` error.
