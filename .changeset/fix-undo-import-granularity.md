---
"loro-crdt": patch
---

Fix `UndoManager` text/list undo and cursor transform depending on how remote updates were split across imports by canonicalizing contiguous `Replace` runs (all inserts before deletes) in `DeltaRope::transform` and `DeltaRope::transform_pos`.
