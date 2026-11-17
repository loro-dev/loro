---
"loro-crdt": patch
---

`UndoManager` undoes and redoes text, list, map and counter edits from inverses recorded when they were committed, instead of checking the document out. The first undo after opening a document no longer builds history caches, parses history or keeps diff trackers resident (for example 6.5 ms and 5.5 MB less for a document with 10,000 maps). Undo results are unchanged.
