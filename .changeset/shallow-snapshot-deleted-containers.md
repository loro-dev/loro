---
"loro-crdt": patch
---

Shallow and state-only snapshots no longer keep containers that were deleted before the shallow root. When the export carried a latest-state overlay (more than 256 retained ops, or any state-only export), the check for "containers created after the root" compared creation ids to the root frontiers for exact equality, so almost every stored container was retained in the root state.
