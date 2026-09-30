# loro.js

## 0.3.0

### Minor Changes

- 408d649: `LoroMovableList` now follows Rust's model: list items (positions) and elements are separate, and an element's position and value are last-writer-wins registers. This fixes convergence with Rust and `loro-crdt`:

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

- c0ff125: Rich text now uses Rust's position model. A mark's start and end anchors are elements of the text sequence, as in Rust (`loro-crdt`), so every insert, delete, and mark position loro.js writes or reads counts them.

  **Breaking:** loro.js reads rich-text history written by earlier versions with Rust's positions. Earlier versions wrote and read positions without the anchors, so every edit made after a mark in a document edited only with loro.js can now apply elsewhere, and styles can cover other text (for example, after marking `ab` in `abcd`, inserting `X` at 3 and deleting 1 gave `acXd` in earlier versions; those ops now read as `bXcd`, as Rust has always read them). Documents shared with `loro-crdt` peers had already diverged there (Rust read `bXcd` from loro.js's updates, loro.js read `acdX` from Rust's); they now agree with Rust. There is no version marker in the data: read "Upgrading from 0.2" in the README before upgrading.

  Behavior now follows Rust:

  - Text inserted where a mark starts or ends goes inside or outside the mark according to the mark's expand setting, as Rust decides it; concurrent edits at a mark boundary converge with Rust.
  - **Breaking:** `mark` and `unmark` throw when `start >= end`, and skip a mark that would change nothing, as Rust does.
  - **Breaking:** internal `_`-prefixed members of `LoroText` that were visible in the type declarations (`_insertVisible`, `_applyMark`, `_styleRuns`, `_validateInsertPosition`, `_unicodePosition`, `_detachedStyleCounter`) were removed.
  - `applyDelta` drops a style that inserted text would inherit when the insert does not list it, like Rust's `apply_delta`.
  - Undo and redo restore the attributes a mark replaced.
  - Snapshots write and read the anchors like Rust, so checking out an older version of a styled text gives the same result in both runtimes. Shallow snapshots null the values of styles that no text is left in, like Rust.
  - Inserting into a nested text that a snapshot loaded lazily no longer throws or drops the styles at the insert position, and checking out a version that has a mark's start but not its end no longer reports the whole text as inserted.

  Upgrading: a snapshot written by an earlier version keeps its current rich text, but its history reads like Rust, so a document loaded from it must not stay in use. Read the final state (`text.toDelta()`) with the old version and build a new document from it (reading it with 0.3 right after importing an old snapshot only works when no updates were stored after that snapshot: with a snapshot of bold `ab` in `abcd` and a later update that inserted `X` at 3 and deleted 1, the old version shows `[a]cXd` and 0.3 and Rust show `[bX]cd`), or import the old updates everywhere and accept Rust's reading. Exporting a shallow snapshot from a document loaded from an old snapshot switches that document itself to Rust's reading.

- e74d64d: Text now follows Rust (`loro-crdt`) everywhere, including when reading documents written by earlier loro.js versions. This breaks documents edited only with loro.js that have concurrent text edits or stored cursors: read "Upgrading from 0.2" in the README before upgrading.

  - **Breaking:** concurrent text inserts are ordered like Rust (the fix is shared with loro-dev/loro#1139). Earlier versions could place an insert after a sibling's subtree behind later, unrelated concurrent text, so a history with such inserts can now read differently (for example `béa9` instead of `bé9a`). Earlier loro.js replicas that merged such edits in different orders, and documents shared with `loro-crdt` peers, had already diverged; they now agree with Rust.
  - **Breaking:** text cursors follow Rust: a cursor reports its target's own offset, so a cursor encoded by an earlier version with side 1, or at the end of the text, resolves one character earlier (the end cursor of `abc` at 2 instead of 3). Other cursors resolve as before. The encoded origin is now the Unicode position, as in Rust. A cursor at the end or in empty text has no target, `getCursorPos` returns the target's own offset for every side, and a deleted target reports the length of the text before it with side `-1`. Get new cursors after upgrading.
  - An imported delete is applied by its position, as Rust does, instead of by its recorded `start_id` (Rust's WASM build up to 1.16.3 can record a wrong one around astral characters, loro-dev/loro#1149).
  - `checkout`, `diff`, and `revertTo` no longer restore a character twice when two concurrent deletes removed it.
  - Consecutive inserts in one transaction are merged into one op, and a delete that spans several ID runs writes the last run first, as Rust does.

  Upgrading: updates and JSON updates written by an earlier version are read like Rust. A snapshot written by an earlier version keeps its current text, but its history reads like Rust, so a document loaded from it must not stay in use (exported updates, replicas loaded from updates, and checkouts can show other content). Exporting a shallow snapshot from such a document switches the document itself to Rust's reading. Migrate every replica the same way: read the final state with the old version and build a new document from it (reading it with 0.3 right after importing an old snapshot only works when that snapshot is the final version, with no updates stored after it), or import the old updates everywhere and accept Rust's reading. See "Upgrading from 0.2" in the README.

### Patch Changes

- 124b70b: `import`, `importBatch` and `importJsonUpdates` are atomic: when an import fails, history, pending changes, version, containers and state are restored, and no event is emitted, so subscribers and `UndoManager` see nothing of it. A multi-blob `importBatch` imports every blob or none.
- 0ab8e38: `checkout(frontiers)` on a `loro.js` document now behaves like `loro-crdt` when `frontiers` is the current version: nothing changes, and the document stays (or becomes) attached when that version is the latest one. Before, `doc.checkout(doc.frontiers())` detached the document, so later imports were not applied to its state. Checking out the latest version from an older one still leaves the document detached, as in Rust.
- 4b04934: `loro.js` now treats a container as deleted when any ancestor is deleted, as `loro-crdt` does: `isDeleted()`, `getPathToContainer()`, and `getCursorPos()` account for deleted ancestors. Descendants of a deleted tree node are deleted: `getNodes()` omits them, and `isNodeDeleted()`/`LoroTreeNode.isDeleted()` return `true`. `LoroTree.nodes()` returns every node, including deleted ones, and `getNodes()` lists alive nodes breadth-first from the roots, matching Rust.
- 106d9e5: `export({ mode: "snapshot" })` on a detached `loro.js` document now encodes the latest state, version, and frontiers, as Rust and `loro-crdt` do. This includes a document imported lazily from a snapshot that received updates while detached. Before, the export wrote the checked-out state and version next to the full history, so an importer saw an old state as the latest version and a later checkout failed. The exporting document keeps its checkout.
- 238988f: `diff()` and `revertTo()` in `loro.js` now restore containers that the range makes reachable again: a map key set back to an older child, a revived list element, or a revived tree node with its whole subtree and metadata, including a node moved out of a deleted ancestor. Their whole state is included, together with nested children, in parent-before-child order, as Rust does. Containers that stay reachable, such as a moved movable-list child, keep only their own changes. `diff()` leaves out containers that are unreachable at the target version.

  Mergeable children follow Rust main (loro-dev/loro#1134). `diff()` reports a re-activated mergeable child with its full state. `applyDiff` applies it incrementally by default, and `applyDiff(diff, { fullState: true })` aligns the receiver's existing child with it: maps per key, counters by difference, text and lists by common prefix and suffix, and trees by node ID. `revertTo` and undo apply only the actual change, so the child keeps its identity. A local re-ensure emits only the parent marker. Import and checkout events now carry the full state of a re-activated child, as in loro-crdt. **An event mirror (subscribe → `applyDiff`) should therefore pass `{ fullState: true }` for `import` and `checkout` batches.** With the default, the full state is applied on top of the mirror's hidden copy.

  Before writing anything, `applyDiff` checks every list delta against the list lengths the batch produces, including a hidden mergeable child's own length. It also dry-runs every tree item, so an out-of-range list delta or a tree item that cannot apply rejects the whole batch. Other malformed entries (text ranges, counter values, a diff whose type does not match its container) can still leave earlier entries applied, as before and as in Rust. Tree items apply in order, as in Rust: a create of a node that is alive moves it, a move of a missing or deleted node creates a new one, and a delete of a deleted node does nothing. Map children are always set as new containers. Tree diffs list moves first (parent first), then the topmost deletes, then revived subtrees, with each index counted in the tree the earlier items leave. A node that is hidden at the target version is reported as deleted. On a movable list, `applyDiff` moves an existing child container instead of creating an empty copy. Diffs, imports and checkouts of deep trees take linear time.

  Undo can revert movable-list moves, including several moves of the same element. The element returns next to its old physical neighbour when that neighbour has not moved since. The undo manager keeps only the tracked ranges its stacks can still use, at a commit cost independent of the stack length. Undoing a move writes a move op, and loro.js still places a remote move by its index rather than its causal position, so a move concurrent with an insert can diverge from Rust (the MovableList model rework, loro-dev/loro#1132).

- e74d64d: Fix a `loro.js` Text/List convergence bug with concurrent inserts. When an insert was concurrent with a run typed after the same character and with another concurrent element whose origin was further left, `loro.js` put the insert after that element instead of at the end of the run. Replicas could disagree with each other (depending on the order in which they received the updates) and with `loro-crdt`. `loro.js` now places these inserts where Rust does.
- b365807: `exportJsonUpdates` now writes movable-list move and set element IDs as `L{lamport}@{peer}`, the format Rust and `loro-crdt` use, and `importJsonUpdates` accepts it. Before, `loro.js` wrote `{lamport}@{peer}`, which Rust rejects, and failed on Rust-produced JSON with `counter is out of range: NaN`. The old form is still accepted on import. The `elem_id` field type is now `JsonIdLp`.
- 18924fa: `exportJsonUpdates` in `loro.js` now writes JSON that Rust and `loro-crdt` accept, in their format:

  - Counter ops carry Rust's `value_type` tag. As in Rust, whose counter is an f64, every counter op is written as `"f64"`, and an `"i64"` value on import is read as an f64. Any finite value round-trips, including the i64 endpoints.
  - Binary values are written as plain number arrays, including the marker bytes of mergeable containers.

  The JSON schema has no binary type, so these arrays come back from `importJsonUpdates` as lists of numbers, in Rust as well. A byte array or a mergeable child therefore does not survive a JSON round trip as binary. Before, `loro.js` passed a `Uint8Array` through, but Rust rejected such JSON.

- 99e7d84: `loro.js` now records the same local ops as `loro-crdt` for these API calls, so a document edited through either package gets the same op IDs and the same concurrent outcomes:

  - `LoroMap.delete(key)` records a delete even when the key is absent. Like in Rust, that delete can win against a concurrent `set` of the same key.
  - `LoroCounter.increment(0)` records an increment, and attaching a detached counter records an increment by its value, even 0.

- 6e3e3c0: Importing a MovableList move or set that names an unknown element, an element of another list, an element outside the op's causal history, or (on a shallow document) an element deleted before the shallow root now fails and rolls back the whole import, as in Rust (loro-dev/loro#1125). A move or set of a deleted element is applied like a concurrent one on every import path.
- 3435bac: Fix a `loro.js` checkout event for a child container in a List when the checkout falls back to replaying history, for example after updates were imported while the document was detached. The List diff could delete and re-insert an unchanged child container without sending its content, so a listener that starts an inserted child from empty lost that child's content. The child's whole state is now sent, as for any other re-attached child.
- c4fd87f: `loro.js` shallow snapshots now pick the same root as `loro-crdt`: the latest single-head critical version of the requested frontiers and the latest version (loro-dev/loro#1095). Before, `loro.js` used the requested version (or the meet of its heads) even when a retained op was concurrent with it. Checking out retained versions of such a snapshot and returning to the latest version could reorder concurrent Text/List elements, and the snapshot disagreed with a Rust export of the same document.
- 4cf1d4a: Shallow snapshots exported by `loro.js` no longer contain containers deleted before the shallow root, such as a map child removed before the cut. Before, every container ever created was written to both the root state and the latest state. Tree node metadata is still kept for all nodes, including deleted ones, because a later move can revive such a node, and every mergeable child is kept, because re-ensuring it after its marker was deleted or replaced shows its state again. Re-exporting an older blob at its own root also drops these containers.
- 1c75b3f: Fix `checkout`, `diff`, and undo on a `loro.js` shallow document when a map value or tree node placement was written in the shallow root commit and changed afterwards. When the root commit held more than one op, retreating to the root dropped the map key or tree node instead of restoring its root-time value, and `getLastEditor` returned `undefined`. A tree node deleted by a retained delete whose placement was trimmed from the shallow history now stays deleted.
- 5e409d1: Fix checkout and `diff` on a `loro.js` document imported from a snapshot. Checking out an earlier version could return the latest value of a child container that had not been read yet, restore nothing for elements deleted before the snapshot, skip a delete when moving forward again, or throw `duplicate sequence id`. A transition now rebuilds only the Text and List containers it touches from their own history, once. Rust-created rich text whose history `loro.js` cannot replay keeps its snapshot state instead, so its latest state and exports no longer change after a checkout; its earlier versions still cannot restore text deleted before the snapshot. Checkout events for containers that had not been read are now correct, and a snapshot exported after history was loaded no longer drops containers that had not been read. A character deleted concurrently by two peers is no longer deleted twice during checkout, and a checkout or `diff` that throws leaves the document unchanged.
- 489088c: Fix `loro.js` local tree moves and creates to match `loro-crdt`:

  - Creating or moving a node between two siblings that share a fractional index (for example two roots created concurrently at index 0) threw `fractional index bounds are not ordered`. It now gives the following siblings with that index new positions in the same transaction, as Rust does.
  - Moving a node to the position it already has, including `move(node, parent)` without an index when it is already last, `node.move(parent)`, and `moveAfter`/`moveBefore` its current neighbor, no longer records an op.
  - `moveAfter` and `moveBefore` within the same parent counted the moved node itself, so moving a node after a later sibling threw `tree index … is out of range` or landed one slot off.
  - Moving a node under one of its own deleted descendants threw `cannot move a tree node below itself or its descendant`. A deleted node's parent is the deleted root, as in Rust, so the move is allowed (and the node becomes deleted).

## 0.2.1

### Patch Changes

- be0e1ac: Fix tree snapshots after `move()` or `delete()` (loro-dev/loro#1088). The tree
  state encoder now writes each parent's children in (fractional index, lamport,
  peer) order, uses the last move/delete operation id, and encodes deleted nodes
  with the default fractional index, matching Rust's tree state layout.
  `snapshot` and `shallow-snapshot` exports from documents whose trees had moved
  nodes previously imported into `loro-crdt` without error, then panicked on the
  first read.

## 0.2.0

### Minor Changes

- 880028d: Store text operations in shared range-backed buffers, read visible spans without
  allocating scalar views, and add lazy line navigation plus explicit text compaction.
- 5ea2e37: Add `pause()`, `resume()`, and `isPaused()` to `UndoManager`. While paused,
  local edits are not recorded as undo steps and checkout events do not clear the
  stacks. Import events (remote changes) are still processed so that the stacks
  remain correctly transformed against concurrent edits. Use this to preserve
  undo/redo history across temporary checkouts such as read-only history previews.

## 0.1.0

### Minor Changes

- 68587bc: Add a pure TypeScript implementation of the current Loro binary format and a
  `loro-crdt`-compatible CRDT runtime.
- 68587bc: Improve pure TypeScript runtime performance for merged changes, concurrent
  sequence insertion, large state snapshots, and bulk list edits. Snapshot
  SSTables now use interoperable LZ4 compression and defer non-frontier history
  decoding until a history-dependent API needs it.

### Patch Changes

- 4e663a1: Keep large latest-state snapshots encoded and hydrate containers on demand.
  Local edits and later updates now use a small history overlay, while snapshot
  export rewrites only dirty SSTable blocks and avoids redundant output buffers.
