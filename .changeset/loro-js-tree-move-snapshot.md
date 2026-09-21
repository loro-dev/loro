---
"loro.js": patch
---

Fix tree snapshots after `move()` or `delete()` (loro-dev/loro#1088). The tree
state encoder now writes each parent's children in (fractional index, lamport,
peer) order, uses the last move/delete operation id, and encodes deleted nodes
with the default fractional index, matching Rust's tree state layout.
`snapshot` and `shallow-snapshot` exports from documents whose trees had moved
nodes previously imported into `loro-crdt` without error, then panicked on the
first read.
