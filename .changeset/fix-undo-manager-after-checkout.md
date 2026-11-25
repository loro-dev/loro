---
"loro-crdt": patch
---

Fix `UndoManager` getting stuck after `checkout()` / `checkoutToLatest()` (`groupStart()` failing with `UndoManager is not ready` and the first local edit after re-attaching being dropped from the undo stack).
