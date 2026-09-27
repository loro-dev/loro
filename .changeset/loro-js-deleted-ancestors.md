---
"loro.js": patch
---

`loro.js` now treats a container as deleted when any ancestor is deleted, as `loro-crdt` does. `isDeleted()` and `getPathToContainer()` account for deleted ancestors, and descendants of a deleted tree node are deleted: `getNodes()` omits them, and `isNodeDeleted()`/`LoroTreeNode.isDeleted()` return `true`. Local edits to a deleted container now throw `The container … is deleted. You cannot apply the op on a deleted container.` instead of recording ops on invisible state. `LoroTree.nodes()` returns every node, including deleted ones, and `getNodes()` lists alive nodes breadth-first from the roots, matching Rust.
