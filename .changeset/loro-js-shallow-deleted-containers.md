---
"loro.js": patch
---

Shallow snapshots exported by `loro.js` no longer contain containers deleted before the shallow root, such as a map child removed before the cut. Before, every container ever created was written to both the root state and the latest state. Tree node metadata is still kept for all nodes, including deleted ones, because a later move can revive such a node, and every mergeable child is kept, because re-ensuring it after its marker was deleted or replaced shows its state again. Re-exporting an older blob at its own root also drops these containers.
