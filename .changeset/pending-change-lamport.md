---
"loro-crdt": patch
---

Fix a panic and order-dependent lamports when a pending remote change gets its deps. The lamport now comes from the deps only, and the lamport in the update is ignored.
