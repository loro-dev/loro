---
"loro-crdt": patch
---

Reduce work per local update and bound event checks during synchronous edits. Speed up short text edits and imports while preserving the existing update and snapshot formats.

Reduce single-operation encoding allocations and speed up concurrent text and list imports.

Avoid parsing cold frontier change blocks during snapshot import and transaction creation, and cut peak WASM heap usage during concurrent text/list merges.
