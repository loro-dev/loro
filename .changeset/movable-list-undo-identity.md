---
"loro-crdt": patch
---

Fix MovableList undo duplicating scalar elements after concurrent moves and losing the gap of a removed element. Undo and redo now use element identities for native moves, sets, and inverse insertion deletes, and keep mappings for elements recreated by a removal undo. Equal scalar values remain distinct elements.

Position restoration prefers the nearest surviving original left neighbour, then the nearest surviving original right neighbour, then the start of the list. The left neighbour wins if neighbours cross; newly inserted elements in the old gap follow the restored element. Undoing a move does not recreate a remotely deleted element. The existing UndoManager no-op skipping policy, ordinary List/Text undo, public diff format, and CRDT merge rules are unchanged.
