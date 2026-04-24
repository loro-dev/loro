---
"loro-crdt": patch
"loro-crdt-map": patch
---

Fix lock-order panics when JavaScript callbacks re-enter Loro APIs.

- `opCount()` no longer reacquires the OpLog lock while the current thread already holds a higher-order lock.
- `LoroText.iter()` snapshots text chunks before invoking the user callback, so callback code can safely read or mutate the document.
