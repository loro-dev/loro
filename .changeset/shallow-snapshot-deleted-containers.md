---
"loro-crdt": patch
---

Shallow and state-only snapshots no longer keep map/list child containers that were deleted before the shallow root. When the export carried a latest-state overlay (more than 256 retained ops, or any state-only export), the check for "containers created after the root" compared creation ids to the root frontiers for exact equality, so almost every stored container was retained in the root state; those snapshots are now smaller.

Tree node metadata is the exception: a tree node deleted before the root can be revived by a later op (for example, moving a child out of a deleted parent), so the meta maps of all tree nodes, including deleted ones, stay in the root state. Shallow snapshots with at most 256 retained ops can therefore be larger than before when a tree has many deleted nodes. This also fixes shallow snapshots where such a revived node lost its metadata, a checkout into the retained history lost the revived node's children, and importing such a snapshot panicked with "Parent is not registered".
