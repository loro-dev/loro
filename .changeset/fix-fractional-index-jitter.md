---
"loro-crdt": patch
"loro.js": patch
"@loro-dev/fractional-index": patch
---

Fix a panic when creating or moving a tree node between siblings whose fractional indexes were generated with jitter (#1211). `newBetween` now also compares the last byte of the shorter index, which jitter makes random, and `newAfter` accepts an all-0xFF suffix. Indexes generated without jitter are unchanged.
