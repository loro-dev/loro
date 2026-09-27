---
"loro-crdt": patch
---

Fix a map value disappearing after importing a shallow snapshot and checking out its root. It happened when a key was rewritten after the root with the value it had at the root (for example by `revertTo`) and the op that wrote the root value was trimmed. Checking out a version also now updates the last editor of map entries and movable-list items whose value is equal at both versions, so `getLastEditor` is correct after a checkout, and shallow and state-only snapshots record the right writer in their root state. Snapshots exported by older versions can still carry the wrong writer in their root state; export them again from a full-history document to fix them.
