---
"loro-crdt": patch
---

Speed up concurrent imports by reusing the current frontier, reducing repeated
ancestor traversal, and filtering unrelated container history before materializing
replay version vectors.
