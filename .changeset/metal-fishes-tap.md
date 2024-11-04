---
"loro-crdt": patch
---

Define the behavior of `doc.fork()` when the doc is detached

It will fork at the current state_frontiers, which is equivalent to calling `doc.fork_at(&doc.state_frontiers())`
