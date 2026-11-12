---
"loro-crdt": patch
---

Merge adjacent text runs with equal effective attributes in `toDelta()` after unmarking. Incremental text events still include null attributes to remove styles.
