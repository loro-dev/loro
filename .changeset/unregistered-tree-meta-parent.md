---
"loro-crdt": patch
---

Fix a crash in documents loaded from a snapshot or created by `fork()`: `isDeleted()`, edits, `getContainerById`, and events touching the metadata of a tree node that was created under an already deleted parent threw `RuntimeError: unreachable` ("Parent is not registered"). The parent of such a container is now found from the change that created it. Also, an import rejected by state validation could leave loaded history pointing at containers and values its rollback had removed, so a later export, checkout, or import crashed (#1161). When such a query needs a change block that is present but cannot be parsed (a corrupted snapshot), the document's current state stays readable, and later `import`, `export` and `checkout` calls throw a "cannot parse change block" error instead of working from a partial history.
