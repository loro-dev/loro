---
"loro-crdt": patch
---

Fix `revertTo`, `applyDiff(diff(a, b))` and undo adding a mergeable child's content twice after its map key was deleted. For example, `revertTo` a version before `map.delete("s")` turned a mergeable text `"hello"` into `"hellohello"`, a counter `7` into `14`, and a list `["keep"]` into `["keep", "keep"]`. The restored child now matches the target version. When its hidden content already matches, no child ops are written.
