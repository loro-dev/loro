---
"loro-crdt": patch
---

Keep a peer's consecutive tree siblings together when another peer inserts its own run into the same gap concurrently. With fractional-index jitter enabled, a sibling created or moved right after the one this peer just placed now takes an index inside that run's range, so two runs begun in one gap no longer interleave (`A1 B1 A2 B2` becomes `A1 A2 B1 B2`). Jittered indexes now end at or above the terminator byte, like unjittered ones, so inserting between a run's members never fails to find an index. Jitter 0 is unchanged.
