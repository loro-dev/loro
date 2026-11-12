---
"loro-crdt": patch
---

Reject malformed change-header counts before allocating memory during import and batch import. Decode change-header and timestamp delta columns with checked bit reads and arithmetic so damaged updates return decoding errors instead of aborting or poisoning the document.
