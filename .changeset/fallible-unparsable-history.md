---
"loro-crdt": patch
---

Return decode errors when fallible history operations first read an unreadable change block, preserving document state instead of panicking during lazy DAG loading.
