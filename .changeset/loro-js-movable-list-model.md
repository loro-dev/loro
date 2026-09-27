---
"loro.js": minor
---

`LoroMovableList` now follows Rust's model: list items (positions) and elements are separate, and an element's position and value are last-writer-wins registers. This fixes convergence with Rust and `loro-crdt`:

- A move of an element that another peer deleted concurrently keeps the element (`["a","c","B"]`, as in Rust), and a delete after a move removes the moved element.
- Snapshots write and read Rust's MovableList layout, including list item IDs, element IDs, last-set IDs and dead list items. Before, loro.js wrote every position and last-set ID as the element ID.
- `getLastEditorAt` and `getLastMoverAt` return the last setter and mover instead of the creator.
- Checkout, `diff`, `revertTo`, `applyDiff` and undo select winners the way Rust does. `revertTo`/`applyDiff` follow Rust's `apply_delta`, and undoing a `move` or `set` reinserts the old value.
- Import and checkout events report the whole state of a child container that a concurrent move brings back.
- `set` records an op even when the value is unchanged, and `move(i, i)` is a no-op, as in Rust. `getIdAt` and `getCursor` of a MovableList anchor the list item, like Rust's cursor.

Also fixed while making loro.js match Rust:

- A document loaded from a snapshot replays history before importing changes that are concurrent with the snapshot. Before, List, Text and MovableList could place such changes at the wrong index.
- Shallow documents accept changes rooted at the empty version when the shallow root is empty, skip changes already inside the shallow root, and a shallow snapshot rooted at a peer's first op now writes the start version that Rust expects.
- Importing a shallow snapshot emits events for containers that exist only in the shallow root state.
