---
"loro-crdt": patch
---

Skip document-wide soft-reclamation statistics when the tracked global heap has no pressure and no local memory policy is configured. Clear WASM reclamation pressure when live heap falls despite retained linear pages. Preserve transaction quota checks across stacked changes, avoid arming rollback scopes inside existing batches or edited transactions, and reserve a conservative UTF-8 bound for large string conversion.
