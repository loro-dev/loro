---
"loro-crdt": patch
---

Perf: optimize importBatch

When using importBatch to import a series of snapshots and updates, we should import the snapshot with the greatest version first.
