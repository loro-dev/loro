---
"loro-crdt": patch
---

Fix `import()` panicking (and poisoning the doc, aborting the process on
drop) when a remote update depends on ops that were folded into a shallow
snapshot. Such updates now fail cleanly with
`ImportUpdatesThatDependsOnOutdatedVersion`, and the unknown-lamport pending
path parks rather than panics if a folded dependency is only discovered at
apply time.
