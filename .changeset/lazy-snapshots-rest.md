---
"loro-js": patch
---

Keep large latest-state snapshots encoded and hydrate containers on demand.
Local edits and later updates now use a small history overlay, while snapshot
export rewrites only dirty SSTable blocks and avoids redundant output buffers.
