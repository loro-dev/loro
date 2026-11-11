---
"loro-crdt": patch
---

Changing the peer id during an active undo group no longer splits the new peer's edits into separate undo steps. The group now restarts from the new peer's counter, so one undo reverts everything the group recorded after the change.
