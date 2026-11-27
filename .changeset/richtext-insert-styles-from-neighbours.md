---
"loro-crdt": patch
"loro.js": patch
---

Text typed locally now takes, for each style, what its displayed neighbours and their expand settings imply (#1240): the value both neighbours share, else the left one's if it expands after, else the right one's if it expands before. Before, the hidden anchors of removed, deleted or split styles could highlight text after an unmark, extend a non-expanding link into a hole, leave text between two adjacent equal links unlinked, or make the result depend on the order of concurrent anchors. When no position among the anchors gives exactly those styles, the insert also writes the mark (or unmark) that does.
