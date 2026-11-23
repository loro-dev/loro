---
"loro-crdt": patch
---

Make charAt and slice on text that has not been decoded since import cost what they read instead of scanning the whole string.
