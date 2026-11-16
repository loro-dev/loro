---
"loro-crdt": patch
---

Fix the last handle of a document skipping its implicit commit when another thread scanned the global memory registry at the same moment (or when two handles were dropped concurrently), which discarded uncommitted edits and their local update. The registry's temporary references and handle drops are now serialized by a per-document drop gate.
