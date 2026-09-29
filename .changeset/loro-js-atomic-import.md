---
"loro.js": patch
---

`import`, `importBatch` and `importJsonUpdates` are atomic: when an import fails, history, pending changes, version, containers and state are restored, and no event is emitted, so subscribers and `UndoManager` see nothing of it. A multi-blob `importBatch` imports every blob or none.
