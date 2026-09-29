# loro.js Tree Diffs: Item Order, Indexes, and Apply

Verified against code 2026-09-29.

How `loro.js` builds and applies tree diff items, and why. The Rust side is in
[tree-checkout-window.md](tree-checkout-window.md) and `crates/loro-internal/src/handler.rs`
(`Handler::apply_diff`, `Self::Tree`).

## Items apply in order

A tree diff is a sequence. Each item's `index`/`oldIndex` refers to the tree that the
earlier items leave, as in Rust. `LoroDoc.#applyTreeDiff` (`loro-js/src/runtime/document.ts`)
applies items in order. It defers an item only while its parent does not exist yet.

Resolution follows Rust's `apply_diff`:

- A `create` of a node that is alive here (the node and all its ancestors are not
  deleted) moves it.
- A `move` of a node that is missing or deleted here creates a new node and remaps its
  meta map.
- A `delete` of a node that is already deleted does nothing.

Placement differs from Rust on purpose. Rust places a node at its `fractionalIndex` and
ignores `index`; loro.js uses `index`. Equal positions (concurrent creates) tie on the new
op's writer, so position placement reorders them. In a random oracle (300 concurrent
histories), position placement lost 30 of 1,200 applies and 47 of 1,200 reverts that
index placement gets right. Consequence: a receiver whose *other* siblings differ from the
source (for example a doc with a different hidden mergeable tree, applied without
`fullState`) orders new nodes differently from Rust.

## How `diff()` orders items

`#completeTreeRevivals` rewrites every tree diff that `diff()` and import/checkout events
produce (via `#completeDiffEntries`):

1. **Moves of live nodes, parent first by depth at `to`.** A node leaves a subtree before
   that subtree is deleted, and no move makes a cycle.
2. **Deletes, topmost only.** These are the diff's own deletes, plus the old copies of
   moved-in nodes that a revived subtree recreates. A node that is hidden at `to` but was
   alive at `from` is emitted as a delete, whatever its recorded item was. A node alive at
   neither version is dropped.
3. **Revived subtrees as creates, parent first.** A revived node's whole subtree is
   recreated, because `diff()` has full-state semantics.

`#sequenceTreeItems` counts each index against the tree the earlier items leave. Only
parents that exist at `from` need counting; a recreated parent gets its children in
order. Under such a parent, the children at any moment are:

- the unchanged ones, counted with the tree's rank query (`LoroTree._childRank`, O(log n))
  minus the changed nodes currently there;
- the present *slots* of changed nodes: a slot at `from` for each changed node that was
  there, and a slot at `to` for each moved or revived node placed there. These are kept in
  a Fenwick tree.

The cost is O(changed · log n), independent of the sibling count. A single move among
100k siblings must stay about 1 ms. Enumerating siblings instead cost 50–75 ms per diff or
event.

## Validation before writing

`applyDiff` dry-runs every tree item with `validateTreeItems`, using the same order,
deferral and resolution as `#applyTreeDiff`. It throws what applying would throw: a
missing parent, a move below itself, or an index outside the parent's children. A
failing batch therefore writes nothing (`loro-js/tests/tree-diff-order.test.ts`). Keep
the two functions in step when changing either one.

## Pitfalls

- Hidden checks walk to the root. Pass a memo (`#isTreeRecordHidden(tree, record,
  cache)`, `#treeNodeAliveAt(..., cache)`), or deep chains go quadratic in `diff()` and in
  import/checkout events with subscribers.
- Build fractional-index hex strings flat (`fractionalIndexHex`). Appending per byte
  leaves rope strings that a 100k-node diff keeps alive, and GC dominates.
- Rust applies a TS diff by fractional index (see above). Diffs with equal positions can
  therefore come out ordered differently in Rust than in loro.js.
