---
"loro-crdt": patch
---

Preserve deleted-container state required by the history retained in `forkAt`
and snapshot-at exports. Historical diffs and checkouts can traverse deleted
list and text descendants without a missing-state panic. Containers created
after the requested frontier remain excluded.

Restore the source frontier after snapshot-at validation errors and preserve
explicit detached mode when exporting a document at its head.
