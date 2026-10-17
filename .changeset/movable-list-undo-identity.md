---
"loro-crdt": patch
"loro.js": patch
---

Fix MovableList undo duplicating scalar elements after concurrent moves and losing the gap of a removed element. Undo and redo now use element identities for native moves, sets, and inverse insertion deletes, and keep mappings for elements recreated by a removal undo. Equal scalar values remain distinct elements.

Position restoration prefers the nearest surviving original left neighbour, then the nearest surviving original right neighbour, then the start of the list. The left neighbour wins if neighbours cross; when it survives, new elements in that gap follow the restored element. Without a surviving left neighbour, restoration goes immediately before the original right neighbour. Undoing a move does not recreate a remotely deleted element. A consumed no-op returns false and leaves other history items for later calls, following the history-boundary fix. Ordinary List/Text undo, public diff format, and CRDT merge rules are unchanged.
