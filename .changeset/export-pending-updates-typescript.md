---
"loro.js": minor
---

Add `LoroDoc.exportPendingUpdates()` to save remote updates waiting for missing
causal dependencies alongside a snapshot. The returned current-format update bytes
can be restored with ordinary `import()`; no pending updates returns `undefined`.
Export leaves pending updates and uncommitted local edits unchanged.
