---
"loro-crdt": patch
---

Fix `unreachable` panic when importing an out-of-order update whose op targets a mergeable child container before its creation (or its parent map) has arrived. Such ops are now buffered as pending and applied once the creating change is imported.
