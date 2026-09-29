---
"loro.js": patch
---

`loro.js` now treats a container as deleted when any ancestor is deleted, as `loro-crdt` does: `isDeleted()`, `getPathToContainer()`, and `getCursorPos()` account for deleted ancestors. Descendants of a deleted tree node are deleted: `getNodes()` omits them, and `isNodeDeleted()`/`LoroTreeNode.isDeleted()` return `true`. `LoroTree.nodes()` returns every node, including deleted ones, and `getNodes()` lists alive nodes breadth-first from the roots, matching Rust.
