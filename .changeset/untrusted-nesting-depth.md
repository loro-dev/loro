---
"loro-crdt": patch
---

Stop deeply nested data from untrusted updates from aborting the process. `LoroValue` equality, container depth tracking, deep-value reads (`getDeepValue`/`toJSON`) and Map/List/MovableList snapshot export no longer recurse once per nesting level, and container depth no longer overflows past 65,535 levels. Mergeable container names whose ancestors' names together exceed 1 MiB are no longer treated as mergeable (they used to cost quadratic time and memory on import), and creating such a child with `ensureMergeable*` returns an error.
