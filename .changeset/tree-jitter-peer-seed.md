---
"loro-crdt": patch
---

Seed a tree's fractional-index jitter from the peer id. Every peer used to draw the same jitter bytes after `enableFractionalIndex(n)` on a fresh tree, so jitter never separated concurrent inserts at the same position (#1289).
