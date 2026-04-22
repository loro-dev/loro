---
"loro-crdt": minor
"loro-crdt-map": minor
---

Make update imports atomic across oplog and document state application.

- `import` and `import_json_updates` now roll back imported oplog changes when state application fails, so malformed updates do not leave the document with oplog/state divergence.
- Pending changes that are activated during import are included in the rollback boundary when they can affect state application.
- Import rollback uses conditional guards to avoid adding fixed overhead to successful detached or no-op imports.
