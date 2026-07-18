---
"loro-js": patch
---

Cut V8 constant factors across hot paths: Number fast paths replace per-bit
BigInt work in the varint and delta column codecs, treap split and fugue
insertion no longer allocate per operation, history records and list elements
keep uniform object shapes, import and commit skip subscriber-only work when
nobody listens, and snapshot encoders drop redundant buffer copies. B4 apply
is ~15% faster and the codec-bound phases are 23–35% faster; the wire format
is unchanged.
