---
"loro-crdt": patch
---

Check encoded list/map counts against available value bytes before reserving their storage, and share a declared-slot budget across nested and successive collections in one value reader. Malformed counts return the existing corruption error earlier; valid encodings and the wire format are unchanged. This bounds these count-driven reservations, not all decoder memory.
