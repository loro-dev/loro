---
"loro-crdt": patch
---

Make undo, redo and checkouts to earlier versions of rich text cost the changes since the last one instead of the whole document history, by keeping the text tracker the first one rebuilds from history and replaying only newer ops into it.
