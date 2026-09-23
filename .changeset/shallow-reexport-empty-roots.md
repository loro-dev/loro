---
"loro-crdt": patch
---

A shallow snapshot re-exported at the document's own shallow root now keeps
root containers that were created after that root and never written to. They
were missing on import when the retained history was short.
