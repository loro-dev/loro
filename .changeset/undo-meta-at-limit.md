---
"loro-crdt": patch
---

Keep `onPush` metadata (`value` and `cursors`) on undo and redo items pushed once the stack holds `maxUndoSteps` items. Each such push evicts the oldest item, so the stack length stays the same, and the metadata was dropped; `onPop` then received `null` and no cursors. The binding now attaches the metadata when the top item's counter span is the pushed span.
