---
"loro-crdt": patch
---

Fix a deadlock when a subscription is dropped and its callback owned another subscription or an `UndoManager` on the same document: subscriber callbacks are now dropped after the subscriber set's lock is released.
