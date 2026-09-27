---
"loro.js": patch
---

`diff()` and `revertTo()` in `loro.js` now restore containers that the range makes reachable again: a map key set back to an older child, a revived list element, or a revived tree node's metadata. Their whole state is included, together with any nested children, in parent-before-child order, as Rust does. Containers that stay reachable, such as a moved movable-list child, and mergeable children, whose state resurfaces when the marker returns, keep only their own changes. `applyDiff` on a movable list now moves an existing child container instead of replacing it with an empty copy.
