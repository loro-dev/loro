---
"loro.js": patch
---

Fix `loro.js` local tree moves and creates to match `loro-crdt`:

- Creating or moving a node between two siblings that share a fractional index (for example two roots created concurrently at index 0) threw `fractional index bounds are not ordered`. It now gives the following siblings with that index new positions in the same transaction, as Rust does.
- Moving a node to the position it already has, including `move(node, parent)` without an index when it is already last, `node.move(parent)`, and `moveAfter`/`moveBefore` its current neighbor, no longer records an op.
- `moveAfter` and `moveBefore` within the same parent counted the moved node itself, so moving a node after a later sibling threw `tree index … is out of range` or landed one slot off.
- Moving a node under one of its own deleted descendants threw `cannot move a tree node below itself or its descendant`. A deleted node's parent is the deleted root, as in Rust, so the move is allowed (and the node becomes deleted).
