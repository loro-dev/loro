---
"loro-crdt": patch
---

Undo and redo text, list, map, counter, tree and movable-list edits from inverses recorded at commit, avoiding history-cache construction for supported steps. Preserve remote edits, checkpoint transforms and existing undo/redo behavior, and trim finite undo byte limits in one pass.
