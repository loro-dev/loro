---
"loro-crdt": patch
---

Make repeated concurrent imports fast: retreat the replay base to the latest
multi-head critical version instead of the single-head one (which is stuck at
the initial fork point on criss-cross sync DAGs), and let the text/list diff
calculators trust a certified-critical base instead of rebuilding their
trackers from the whole history on every concurrent import.
