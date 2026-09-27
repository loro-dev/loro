---
"loro-crdt": patch
---

Fix panics involving containers of a type this version doesn't know (created by a newer Loro):

- `applyDiff` no longer traps the WASM instance when a diff inserts such a container (e.g. `"🦜:cid:0@1:Unknown(9)"`) into a List, MovableList, Map or tree node meta. It now throws a readable error before changing the doc. `revertTo` that would have to recreate such a container throws the same error, and an undo step that would have to recreate one is skipped without changes (like other undo steps that fail to apply). Moves of unknown elements that are already in a MovableList keep working.
- Importing or checking out updates that create an unknown container and edit it in the same range no longer traps.
- `getCursorPos` returns `undefined` for cursors on Map, Tree, Counter or unknown containers instead of trapping.
- `parent()` of a container whose parent has an unknown type throws a readable error instead of trapping.
- Undo/redo no longer traps when an `onPush` cursor points to a missing container.
