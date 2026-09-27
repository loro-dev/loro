---
"loro-crdt": patch
---

Fix `revertTo`, `applyDiff(diff(a, b))` and undo adding a mergeable child's content twice after its map key was deleted. For example, `revertTo` a version before `map.delete("s")` turned a mergeable text `"hello"` into `"hellohello"`, a counter `7` into `14`, and a list `["keep"]` into `["keep", "keep"]`. The restored child now matches the target version and keeps its characters, list elements and tree node ids, so only the real difference is written. `diff()` now returns a delta relative to the hidden child's state for a re-activated mergeable child, not its full content. Checkout and import events are unchanged.
