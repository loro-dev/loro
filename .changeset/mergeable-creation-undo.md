---
"loro-crdt": patch
---

Fix undoing a step that creates and edits a mergeable container so its content does not reappear when the same container is ensured again. Preserve concurrent edits and existing hidden content across undo and redo.
