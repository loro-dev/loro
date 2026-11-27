---
"loro-crdt": patch
---

Fix `UndoManager` throwing `Cycle move` forever when undoing a tree move whose old parent was concurrently moved under the node. The cyclic move is now skipped, so older history stays reachable.
