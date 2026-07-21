---
"loro-crdt": minor
"loro-js": minor
---

Add `pause()`, `resume()`, and `isPaused()` to `UndoManager`. While paused,
local edits are not recorded as undo steps and checkout events do not clear the
stacks. Import events (remote changes) are still processed so that the stacks
remain correctly transformed against concurrent edits. Use this to preserve
undo/redo history across temporary checkouts such as read-only history previews.
