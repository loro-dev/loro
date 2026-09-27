---
"loro.js": patch
---

Shallow snapshots exported by `loro.js` no longer contain containers deleted before the shallow root, such as a map child removed before the cut. Before, every container ever created was written to both the root state and the latest state. Tree node metadata is still kept for all nodes, including deleted ones, because a later move can revive such a node. Re-exporting an older blob at its own root also drops these containers. Retreating a retained tree delete whose placement op was trimmed from the shallow history now keeps the node deleted.
