---
"loro-crdt": patch
---

Fix containers that stayed deleted after being revived. After `isDeleted()` was called on the metadata of a tree node under a deleted ancestor, moving the subtree back (locally or through an import) left that metadata reported as deleted, and edits to it threw until the document was reloaded. The same happened to a movable-list child revived by importing a concurrent move. Cached deletions of tree metadata and movable-list children are now dropped whenever a tree move or a movable-list change is applied.
