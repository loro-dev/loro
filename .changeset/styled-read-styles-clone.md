---
"loro-crdt": patch
"loro-crdt-map": patch
---

Fix an O(n^2) slowdown when reading styled text.

Building the per-range style metadata for `toDelta` / `getRichtextValue` (and
the sliced variants used to emit text events) deep-copied the whole `Styles`
map once per style range. Each key in that map owns the set of *every* style op
covering the range, while the resulting `StyleMeta` keeps only the LWW winner,
so the copy scaled with the number of marks accumulated on the container and
then discarded all but one element of it — O(marks) per range across O(marks)
ranges. The conversion now borrows instead of cloning.

Style anchors are never consolidated, so marks accumulate in the container
state and every styled read paid for all of them. On a 724-character paragraph
carrying 2000 accumulated marks, a styled read drops from ~103ms to ~1ms; a
simulated editor binding that re-asserts marks on each keystroke drops from
~5ms to ~200us per read after 500 keystrokes.
