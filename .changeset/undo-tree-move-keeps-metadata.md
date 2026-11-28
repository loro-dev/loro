---
"loro-crdt": patch
---

Keep a tree node's metadata when undoing a move that brings the node back after a remote delete. The recorded inverse is only the move, so a missing map entry is no longer treated as an empty full state.
