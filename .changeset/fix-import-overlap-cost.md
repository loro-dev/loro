---
"loro-crdt": patch
---

Reduce known-history comparison costs for snapshot-loaded documents while preserving peer-id reuse detection. Snapshot updates allocate only new history in the document arena and avoid cloning discarded changes.

Avoid unnecessary rollback scopes for detached Text imports while retaining validation of pending movable-list operations.

Keep import rollback decisions consistent across concurrent attach/detach, and fall back to ordinary history parsing when a cold-block comparison is ineligible.
