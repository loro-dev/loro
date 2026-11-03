---
"loro-crdt": patch
---

Speed up importing a concurrent change into a document that has seen many peer ids. Turning the replay base into frontiers walked the history back once per peer; the walk now stops at nodes whose version vector is already cached, so a document with hundreds of peers imports in a fraction of a millisecond instead of tens to hundreds.
