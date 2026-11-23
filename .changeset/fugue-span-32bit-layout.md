---
"loro-crdt": patch
---

Allow smaller `FugueSpan` layouts on 32-bit targets while retaining the 64-byte memory bound. This fixes native i686 musl builds without changing the wire format or runtime behavior. Publish the Rust fix as `loro-internal` 1.19.1, compatible with `loro` 1.19.0.
