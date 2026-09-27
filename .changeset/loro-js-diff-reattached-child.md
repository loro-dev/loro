---
"loro.js": patch
---

`diff()` and `revertTo()` in `loro.js` now restore containers that the range makes reachable again: a map key set back to an older child, a revived list element, or a revived tree node together with its whole subtree and metadata, including a node moved out of a deleted ancestor. Their whole state is included, together with nested children, in parent-before-child order, as Rust does. Containers that stay reachable, such as a moved movable-list child, and mergeable children, whose state resurfaces when the marker returns, keep only their own changes. `applyDiff` on a movable list now turns a delete and re-insert of the same child container into a single move, as Rust's `apply_delta` does, and undo can revert movable-list moves.
