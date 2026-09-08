---
"loro-crdt": patch
"loro-crdt-map": patch
---

Make imports that are concurrent with the current version only on map/counter
containers as fast as ordinary linear imports.

Previously any concurrency between the new ops and the current version
(for example a single unmerged head from another peer) forced every later
import to retreat to a critical version and replay the whole branch, and the
map diff scanned every key of every touched map. Per-import cost grew with
both the branch length and the map size: 400 small imports on a 2000-note doc
with one stale head took ~2.4s natively; they now take ~70ms, flat per import
(a linear import of the same chain takes ~50ms).

The oplog now proves, per container, whether the concurrent old history
touches anything that needs positional context. When it only touches registers
the import replays from the current version; the affected maps are still
resolved from history so persisted state that drops tombstones (deleted roots,
dead containers) cannot influence the result. Map diffs in the conservative
path are also restricted to the keys the update touched.
