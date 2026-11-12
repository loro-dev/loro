---
"loro-crdt": patch
---

Avoid recursive parent queries in the tree history cache so deep trees do not
exhaust the stack. Add regression coverage for historical operations after
concurrent tree moves, including the skipped-cycle case from #1117 whose
replay ordering was fixed in #1152.
