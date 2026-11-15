---
"loro-crdt": patch
---

Keep the requested frontier as the shallow snapshot root when it is a critical version. When another peer wrote both before and after it (for example two peers taking turns), the root used to retreat far below it, often to near the start of the history, so the snapshot kept history it did not need.
