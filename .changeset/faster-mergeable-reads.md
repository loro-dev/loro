---
"loro-crdt": patch
---

Speed up documents with many mergeable children. The arena caches each mergeable child's parsed edge and activating marker, so liveness checks, map reads, deep values and container-tree reads no longer re-parse cids or re-hash markers per op or per child; the marker CRC is table-driven and allocation-free, and map lookups no longer intern their key. With 100k mergeable children: local edits ~2.3x faster, `get_deep_value` ~1.8x, building the document ~1.6x.
