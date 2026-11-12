---
"loro-crdt": patch
---

Apply pending updates after initializing full or shallow snapshots, keeping document state and history aligned. Preserve lazy snapshot state and empty roots, and restore pending changes without emitting events when snapshot import or batch validation fails.
