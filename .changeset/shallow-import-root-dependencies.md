---
"loro-crdt": patch
---

Reject updates concurrent with the shallow root frontier with
ImportUpdatesThatDependsOnOutdatedVersion instead of queuing them and panicking
while resolving a dependency that has already been trimmed. Rejected updates do
not enter pending storage; subsequent valid post-root updates still apply.
