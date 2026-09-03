---
"loro-crdt": patch
---

Fix the richtext diff calculator's style table growing a duplicate entry for
every re-replayed style op (e.g. checkout walks after a retreat).
