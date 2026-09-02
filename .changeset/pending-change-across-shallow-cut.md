---
"loro-crdt": patch
---

A change parked as pending before the doc imported a shallow snapshot, whose deps
the snapshot then trimmed, no longer aborts the process when a later import
unlocks it. It is dropped and that import returns
`ImportUpdatesThatDependsOnOutdatedVersion`, the same outcome as importing it
after the doc became shallow.
