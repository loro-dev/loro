---
"loro-crdt": minor
"loro-crdt-map": minor
---

Add `LoroDoc.exportPendingUpdates()` (`export_pending_updates()` in Rust) to
persist remote updates waiting for missing causal dependencies. It returns
`undefined` in JavaScript (`None` in Rust) when there are no pending operations.
The result uses the existing updates format and can be restored with ordinary
`import` after a separately saved snapshot. Export is read-only and does not
commit local transactions. Ordinary snapshot/update exports continue to exclude
remote pending operations.
