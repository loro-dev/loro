---
"loro-crdt": patch
---

Fix `getContainerById` / `hasContainer` for ensured-but-empty mergeable containers.
After `ensureMergeableMap` (and friends), the child was visible via `map.get(key)`
and `toJSON()` but its id did not resolve until the first op was written into it —
locally and on remote peers after sync. The id now resolves as long as the parent
map's child ref is alive; a mergeable cid that was never ensured still resolves to
`undefined`.
