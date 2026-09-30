---
"loro-crdt": patch
---

Reject imported Text ops whose positions lie past the end of the text. An insert, delete or mark past the end used to panic inside the document (a `RuntimeError: unreachable` trap in WASM, leaving the doc unusable). `import`, `importBatch` and `importJsonUpdates` now throw a decode error and leave the document unchanged and usable, like out-of-bounds List and MovableList ops (#1160).
