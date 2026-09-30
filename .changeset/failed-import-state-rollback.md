---
"loro-crdt": patch
---

After an import failed and was rolled back, containers created later could show state left over from containers the failed import had touched (for example a new root map showing a tree node's metadata), read as deleted, or trip an internal assertion when the same bad update was imported again. A failed import now leaves the document's containers unchanged.
