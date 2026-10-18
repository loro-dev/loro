---
"loro-crdt": patch
---

Reject encoded cursors with negative position counters with a readable decode error. Cursor queries also reject directly constructed invalid cursors before accessing document history, preventing panics and poisoned document locks. Valid and unknown positive cursor positions keep their existing behavior.
