---
"loro-crdt": patch
---

Compress snapshot SSTable blocks with lz4rip instead of lz4_flex. The LZ4 frame format does not change, so older versions read new snapshots. Snapshots are up to 5% smaller, and reading them after import is a little faster.
