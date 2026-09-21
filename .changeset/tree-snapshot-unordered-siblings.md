---
"loro-crdt": patch
---

Accept tree snapshot state whose sibling nodes are not in fractional index
order, such as `snapshot`/`shallow-snapshot` exports from loro.js 0.1.0/0.2.0
after a tree `move()` (loro-dev/loro#1088). The decoder now sorts siblings by
their (fractional index, idlp) position instead of panicking with
`assertion failed: last.0 < pos` on the first read. Tree state with duplicate
node ids or duplicate sibling positions is rejected as a decode error.
