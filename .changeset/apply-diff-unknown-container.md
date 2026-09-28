---
"loro-crdt": patch
---

Fix panics and lost events involving containers of a type this version doesn't know (created by a newer loro-crdt):

- `applyDiff` and `revertTo` no longer trap the WASM instance when they would have to create such a container, e.g. a diff that inserts `"🦜:cid:0@1:Unknown(9)"` into a List, MovableList, Map or tree node meta, or that recreates a deleted container holding one. They throw a readable error and leave the doc unchanged. Moves of unknown elements that are already in a MovableList keep working. With `{ fullState: true }`, re-activating a mergeable child that still holds such a container keeps it.
- `UndoManager.undo()`/`redo()` throw the same error, without changing the doc, for a step that would recreate such a container. That step is dropped and its edits stay; the next call undoes exactly the step before it (previously the step was applied partially, losing data, and reported success).
- Importing or checking out updates that create an unknown container and edit it in the same range no longer traps.
- `subscribe` callbacks no longer lose a whole event batch when it touches an unknown container; only the events that can't be represented are left out.
- `getCursorPos` returns `undefined` for cursors on Map, Tree, Counter or unknown containers instead of trapping.
- `parent()` of a container whose parent has an unknown type throws a readable error instead of trapping.
- Undo/redo no longer traps when an `onPush` cursor points to a missing container.
