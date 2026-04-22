---
"loro-crdt": patch
"loro-crdt-map": patch
---

Fix several edge-case contract violations in document, text, and JSONPath APIs.

- JSONPath `value(...)` comparisons now handle boolean values consistently with other scalar comparisons.
- Rich text mark expansion now follows `ExpandType::Before` and `ExpandType::Both` at documented insertion boundaries.
- Text delta slicing now validates invalid ranges and UTF-8/UTF-16 boundaries before slicing, and public deltas omit removed-style tombstones after unmarking.
- Detached list and movable-list out-of-bounds operations now return `LoroError::OutOfBound` instead of panicking.
