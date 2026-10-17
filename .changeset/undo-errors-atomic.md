---
"loro-crdt": patch
---

Undo and redo return application errors after rolling back the failed inverse, preserving document state, the history item and its remote transform base, and container/MovableList identity mappings for retry. Failed operations emit no events. Compound undo operations serialize against transaction writers.

Keep the existing unknown-container policy: a step rejected before application is consumed, leaves the document unchanged, and returns an error without trying another item.
