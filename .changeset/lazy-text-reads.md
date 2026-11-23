---
"loro-crdt": patch
---

Make charAt, slice and iteration on text that has not been decoded since import cost what they read instead of scanning the whole string, and stop copying every chunk before iterating.
