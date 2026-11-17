---
"loro-crdt": patch
---

`UndoManager` undoes and redoes text, list, map, counter and tree edits (node creation, moves between live parents, deletion of a leaf without metadata) from inverses recorded when they were committed, instead of checking the document out. The first undo after opening a document no longer builds history caches, parses history or keeps diff trackers resident: for example 6.5 ms and 5.5 MB less for a document with 10,000 maps, and 80 ms and 22.7 MB less for a 40,000-node tree. Undo results are unchanged.
