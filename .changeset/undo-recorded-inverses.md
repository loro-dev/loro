---
"loro-crdt": patch
---

`UndoManager` undoes and redoes text, list, map, counter, tree and movable-list edits from inverses recorded when they were committed, instead of checking the document out. The first undo after opening a document no longer builds history caches, parses history or keeps diff trackers resident: for example 6.5 ms and 5.5 MB less for a document with 10,000 maps, 80 ms and 22.7 MB less for a 40,000-node tree, and 2.4 MB less for a 100,000-element movable list. Undo results are unchanged.
