---
"loro-crdt": patch
---

Fix a checkout hang after snapshot import. Change-store blocks decoded from a snapshot recorded a wrong end lamport (`lamport_range.1` was set to the start lamport of the block's last change instead of its end). When a change was split across multiple blocks and a lamport-based lookup engaged the binary search path in `ChangeStore::get_change_by_lamport_lte` — e.g. the movable-list diff calculator resolving historical positions during `checkout` — the degenerate range made the search loop forever. The binary search now also caps its steps and falls back to scanning the underlying kv store if it ever fails to converge.
