---
"loro-crdt": patch
---

Reduce repeated transaction bookkeeping allocation costs, share document ownership
between related container handlers, and defer materialization of single text
insertion inverses until undo needs them.

Coalesce continuous text insertion runs within transactions and undo groups.
Mixed text replacement inverses fall back to checkout to preserve their ordering
relative to later remote edits.
