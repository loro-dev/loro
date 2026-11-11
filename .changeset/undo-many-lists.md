---
"loro-crdt": patch
---

Make undoing a commit that created many lists fast: a retreat (undo, or a
checkout to an older version) now certifies its target as a critical replay
base, and the list and movable-list diff calculators trust a tracker seeded
there instead of rebuilding it from the whole history once for every list
the undone span touched.
