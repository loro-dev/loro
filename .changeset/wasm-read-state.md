---
"loro-crdt": minor
---

Add `LoroDoc.readState()` for reading nested container snapshots with explicit `type`, `cid`, and `value` fields. Ordinary values are opaque `Value` nodes, so Map/List data cannot be confused with containers. The same API reads an individual container or a clamped list interval. Text can return plain strings or formatting deltas; Tree node metadata preserves its Map ID and nested containers. JavaScript values are built directly with fixed constructors, per-read key/peer reuse, and owned binary buffers.
