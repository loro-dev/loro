---
"loro-crdt": patch
---

Count the values held by map, list-set, style and list-insert ops in `memoryStats()` (`uncommittedTxnBytes`, `changeCacheBytes`) and in `maxDocBytes` admission. Overwriting a key with large values in one transaction previously bypassed the document quota.
