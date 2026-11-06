---
"loro-crdt": patch
---

Reduce memory use: importing an update into a document opened from a snapshot no
longer loads its whole history, version vectors keep sharing structure when merged,
small tables reserve smaller block caches, and parsed ops keep no spare capacity.
