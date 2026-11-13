---
"loro-crdt": patch
---

Avoid allocating a traversal stack for scalar value comparisons, removing repeated allocations when merging consecutive rich-text insertions with the same styles. Deep value comparisons remain non-recursive.
