---
"loro-crdt": patch
---

Fix grouped undo across split history spans restoring deleted text out of order
or reviving intermediate text after a causally dependent remote update. Compose
inverse map writes in execution order so repeated writes to one key restore its
pre-group value. Preserve remote edits and redo behavior.
