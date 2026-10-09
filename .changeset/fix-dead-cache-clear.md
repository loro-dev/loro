---
"loro-crdt": patch
---

Fix quadratic local tree moves when interleaved with queries or failed edits on deleted containers by releasing the revivable deletion cache's table on invalidation.
