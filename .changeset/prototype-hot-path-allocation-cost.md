---
"loro-crdt": patch
---

Reduce repeated transaction bookkeeping allocation costs, share document ownership
between related container handlers, and defer materialization of single text
insertion inverses until undo needs them.
