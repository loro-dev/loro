---
"loro-crdt": patch
---

Without an allocator tracker, the global memory estimate now recomputes only documents that changed since the last check instead of every live document, so a budgeted commit no longer slows down with the number of open documents. A document whose soft reclamation could not reach its target watermark no longer repeats it on every commit until its usage grows. Importing a shallow snapshot no longer keeps the whole input buffer allocated after the history is rewritten.
