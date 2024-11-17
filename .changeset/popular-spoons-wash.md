---
"loro-crdt": patch
---

The fractional index in LoroTree is now enabled by default with jitter=0.

To reduce the cost of LoroTree, if the `index` property in LoroTree is unused, users can still
call `tree.disableFractionalIndex()`. However, in the new version, after disabling the fractional 
index, `tree.moveTo()`, `tree.moveBefore()`, `tree.moveAfter()`, and `tree.createAt()` will 
throw an error

