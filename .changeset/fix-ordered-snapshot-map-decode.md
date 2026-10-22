---
"loro-crdt": patch
---

Avoid an intermediate hash map when first reading snapshot-backed Map values. Reuse a single postcard cursor and decode directly into the ordered lazy cache while preserving iterative deep-value stack safety, the existing binary format, and error handling for malformed input.
