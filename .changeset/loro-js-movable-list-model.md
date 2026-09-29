---
"loro.js": minor
---

`LoroMovableList` now follows Rust's model: list items (positions) and elements are separate, and an element's position and value are last-writer-wins registers. This fixes convergence with Rust and `loro-crdt`:

- A move of an element that another peer deleted concurrently keeps the element (`["a","c","B"]`, as in Rust), and a delete after a move removes the moved element.
- Snapshots write and read Rust's MovableList layout, including list item IDs, element IDs, last-set IDs and dead list items. Before, loro.js wrote every position and last-set ID as the element ID.
- `getLastEditorAt` and `getLastMoverAt` return the last setter and mover instead of the creator.
- Checkout, `diff`, `revertTo`, `applyDiff` and undo select winners the way Rust does. Undoing a `move` or `set` reinserts the old value, and undo transforms its inverse only over changes the `UndoManager` did not make, as Rust's does.
- `set` records an op even when the value is unchanged, and `move(i, i)` is a no-op, as in Rust. `getIdAt` and `getCursor` of a MovableList anchor the list item, like Rust's cursor.
- Importing concurrent moves resolves their indices incrementally, like Rust's tracker, instead of one causal view per op.

Also fixed while making loro.js match Rust:

- A document loaded from a snapshot rebuilds each container that an import touches concurrently with the snapshot from that container's history first (as a checkout already does), so List, Text and MovableList place such changes where Rust does (loro-dev/loro#1163). A MovableList snapshot takes part in this completion like Text and List.
- Changes imported as pending while a snapshot's history was still undecoded were lost once the history was decoded (for example by a checkout or a history query); they now stay pending.
- Shallow documents follow Rust's rule for outdated imports: they accept changes rooted at the empty version when the shallow root is empty, skip changes already inside the shallow root, and reject a change whose part after the root depends on trimmed history. A shallow snapshot rooted at a peer's first op now writes the start version that Rust expects.
- An import that only adds pending changes creates the root containers they edit, as Rust does.
- Importing a shallow snapshot emits events for containers that exist only in the shallow root state.
