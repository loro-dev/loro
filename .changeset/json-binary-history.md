---
"loro-crdt": patch
---

Preserve binary values across JSON operation export and import. A document that receives this JSON can subsequently import the original writer's overlapping binary update with the default strict conflict check.
