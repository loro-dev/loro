---
"loro-crdt": patch
---

Fix `forkAt`/`checkout` becoming quadratic on large documents. `find_common_ancestor`
discarded the common ancestor it had just computed and returned empty frontiers
whenever any branch of its DAG walk ended without meeting the other side. Checking
out to a version that is already in the document's past — exactly what
`forkAt(frontiers)` does — always ends that way, so the diff calculator was told to
replay from the very beginning of history. That rebuilds the tracker over the whole
oplog for *every* container in the document, which scales quadratically and looks
like a frozen tab in the browser.

The conservative empty-frontiers fallback is now skipped when the computed ancestor
is exactly the target version. In that case every id of the target is reachable from
the source too, so the target already *is* the maximal common ancestor and there is
nothing to be conservative about. It is also the safe direction for the diff
calculators: the source version then strictly contains the target, so the retreat is
never empty and the richtext/list/movable-list trackers are still rebuilt from CRDT
ids rather than advanced incrementally. On a 1600-container / 194k-op document this
takes `checkout` from 268ms to 1.4ms and `forkAt` from 679ms to 337ms.
