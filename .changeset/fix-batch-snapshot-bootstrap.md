---
"loro-crdt": patch
---

Use the lazy fast-snapshot import path when a multi-blob `importBatch` starts with a full snapshot in an empty, attached document, then apply the remaining updates in one checkout. Preserve batch rollback on closing validation or checkout failure, including the snapshot's state and history, and defer snapshot events until checkout succeeds.
