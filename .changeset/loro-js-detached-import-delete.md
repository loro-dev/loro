---
"loro.js": patch
---

Fix `loro.js` losing Text/List deletions that were imported while the document was detached. After `checkout(...)`, importing an update that deleted characters or list items, and then calling `checkoutToLatest()` or `checkout(...)` to a version that includes the delete, kept the deleted content visible. It now matches `loro-crdt` and a fresh import of the same updates.
