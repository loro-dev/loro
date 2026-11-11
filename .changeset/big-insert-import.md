---
"loro-crdt": patch
---

Fix importing one big text insert made right after a concurrent change taking quadratic time (3M characters: ~50 s, now ~0.1 s). The text tracker updated the id-to-cursor map once per rope leaf report, and the rope reported the same leaf once per 256-op fragment; each update walked the whole insert. Repeated leaves are now updated once.
