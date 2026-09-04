---
"loro-crdt": patch
---

Importing into a shallow doc an update whose deps are the shallow root's own
deps, or that mix the root with a trimmed id of another peer, now returns
`ImportUpdatesThatDependsOnOutdatedVersion` instead of panicking (and aborting
the process through a poisoned doc mutex). Such an update is concurrent with
the shallow root, so it is rejected like any other update that branches off
trimmed history.
