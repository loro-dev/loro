---
"loro-crdt": patch
---

Avoid stack overflow when first reading or dropping deeply nested map/list values from small snapshots. Decode snapshot values, release their shared storage, and convert them to JavaScript iteratively. Preserve the snapshot format and lazy import behavior. Other serde deserialization of LoroValue now rejects nesting beyond 128 levels with an error.

Rich-text mark values in binary snapshots and shallow style redaction also use the iterative codec, retaining values deeper than the external 128-level serde limit. Keep deep mark export and style hashing off the call stack so 20,000-level marks round-trip on small native stacks. The object JSON import restriction is unchanged.
