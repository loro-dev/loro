---
"loro-crdt": patch
---

Fix a crash in documents loaded from a snapshot or created by `fork()`: `isDeleted()`, edits, `getContainerById`, and events touching the metadata of a tree node that was created under an already deleted parent threw `RuntimeError: unreachable` ("Parent is not registered"). The parent of such a container is now found from the change that created it. Also, a failed import in such a document could leave the history pointing at the wrong containers, so a later export crashed.
