---
"loro-crdt": patch
---

Release superseded snapshot buffers, preserve warm parsed history during ordinary exports, and avoid redundant JavaScript typed-array copies during batch import. Restore tombstone filtering and small-block coalescing during incremental KV export, and allocate export-version metadata lazily.
