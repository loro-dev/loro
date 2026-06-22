---
"loro-crdt": patch
---

Improve snapshot import performance by skipping eager SSTable block metadata validation on fast imports while still verifying block checksums.
