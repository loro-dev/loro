---
"loro-crdt": patch
---

Fix a convergence bug where a movable tree's incrementally maintained state
could diverge from a full replay of its own oplog. When newly imported
operations were concurrent with part of the receiving peer's multi-head
frontier, the diff mode was misclassified as concurrency-free and the tree
fast path applied the new moves without adjudicating them against the
existing concurrent branch. The classifier now verifies that every entry
point of the imported region causally covers the whole current frontier,
and otherwise retreats the replay base to the latest critical version so
the competing branches are replayed together.
