---
"loro-crdt": patch
---

Fix missing tree events on import. When an imported move brought a node back out of a deleted subtree, only the node got a `create` event, not the nodes below it, so a tree rebuilt from events lost them. Importing a move of a node under a deleted node also emitted no `delete` event in some cases. This happened when the import had no concurrency with the local document, for example on a peer that only receives updates.
