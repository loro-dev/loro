---
"loro-crdt": patch
---

Reject out-of-range signed DeltaInt payloads as corrupted data instead of silently narrowing them to i32. Valid i32 values and accepted nonminimal signed varints keep their existing decoding behavior.
