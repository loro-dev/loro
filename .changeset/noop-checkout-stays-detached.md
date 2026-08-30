---
"loro-crdt": patch
---

Fix `checkout()` to the frontiers a detached doc is already at silently
re-attaching the doc (1.13.6 -> 1.13.7 regression). Detached is an explicit
mode: it is entered via `checkout` and left via `checkoutToLatest`/`attach`.
The no-op early return no longer derives the attached flag from the oplog
frontiers, so `isDetached()` at HEAD no longer depends on how the doc got
there.
