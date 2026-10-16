---
"loro-crdt": patch
---

Avoid stack overflow when first reading or dropping deeply nested map/list values from small snapshots. Decode snapshot values, release their shared storage, and convert them to JavaScript iteratively. Preserve the snapshot format and lazy import behavior. Other serde deserialization of LoroValue now rejects nesting beyond 128 levels with an error.
