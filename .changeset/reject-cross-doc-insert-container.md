---
"loro-crdt": patch
---

Reject inserting a container attached to another `LoroDoc` (directly or nested inside a detached container) with a recoverable error instead of trapping the WASM instance. The target doc is left unchanged.
