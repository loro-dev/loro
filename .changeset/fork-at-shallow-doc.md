---
"loro-crdt": minor
---

`forkAt` and snapshot-at exports now work on shallow documents for any
frontier at or after the shallow root. The fork is itself a shallow document
with the same shallow root, so it keeps syncing with its source. Frontiers
before the shallow root still fail, because their history is trimmed.
