---
"loro-crdt": patch
---

Avoid stack overflows when reading a deeply nested LoroTree imported from a snapshot. Use iterative tree hierarchy, ancestor, deletion-status, snapshot, metadata, and value traversals, and release nested tree-node results iteratively while preserving node order and document contents. This builds on the shared-value stack safety in #1204.
