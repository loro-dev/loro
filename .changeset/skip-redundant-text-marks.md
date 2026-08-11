---
"loro-crdt": patch
"loro-crdt-map": patch
---

Fix unbounded degradation when re-asserting identical text marks over styled ranges.

`mark` already tried to skip no-op marks, but the check used
`StyleRangeMap::get_styles_of_range`, which returns `None` whenever the marked
range spans more than one style-range leaf. On any document whose style ranges
are already fragmented (i.e. any document with styles), re-asserting an
identical mark recorded a new op every time.

Each redundant mark leaves a pair of style anchors in the container state
forever — they survive snapshots and are never consolidated — and every styled
read walks all of them. Callers that re-assert marks (for example editor
bindings syncing mark state) therefore permanently degraded styled reads without
bound: reading a 724-character paragraph went from ~5µs to ~4ms after 1000
redundant marks.

The skip path now falls back to `StyleRangeMap::range_has_key_value`, which
scans every style range covering the mark and reports whether each position
already resolves the key to the given value. Attached and detached texts both
use the new path when the single-leaf fast path cannot answer.
