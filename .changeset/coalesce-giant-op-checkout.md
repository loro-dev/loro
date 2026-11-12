---
"loro-crdt": patch
---

Speed up `checkout`, `diff`, `forkAt`, and concurrent imports across a single large text insert or delete by coalescing contiguous same-effect leaf updates in the rich-text tracker instead of splitting and re-merging the rope leaf at every 256-character fragment boundary.
