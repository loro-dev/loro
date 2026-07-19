---
"loro-js": patch
---

Keep semantic no-op detection type-safe so empty arrays, objects, maps, binary
values, and containers cannot be mistaken for one another. Match Rust by
rejecting `isNodeDeleted()` queries for tree nodes that do not exist and by
preserving deleted-tree roots, descendants, positions, and last-move metadata
with the same observable semantics.
