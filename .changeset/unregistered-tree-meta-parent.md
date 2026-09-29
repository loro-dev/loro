---
"loro-crdt": patch
---

Fix a crash in documents loaded from a snapshot or created by `fork()`: `isDeleted()`, edits, `getContainerById`, and events touching the metadata of a tree node that was created under an already deleted parent threw `RuntimeError: unreachable` ("Parent is not registered"). The parent of such a container is now found from the change that created it. Also, an import rejected by state validation could leave loaded history pointing at containers and values its rollback had removed, so a later export, checkout, or import crashed (#1161). A change block that is present but cannot be parsed now fails with an internal error when a query needs it, instead of the container reading as deleted.
