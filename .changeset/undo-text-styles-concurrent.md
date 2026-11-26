---
"loro-crdt": patch
---

`UndoManager` no longer overwrites a collaborator's concurrent text formatting: undoing or redoing a mark keeps a style key another peer set or removed afterwards (#1229), and undoing a mark also removes it from text a peer inserted concurrently inside the marked range (#1009).
