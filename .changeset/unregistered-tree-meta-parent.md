---
"loro-crdt": patch
---

Fix a crash in documents loaded from a snapshot or created by `fork()`: `isDeleted()`, edits, `getContainerById`, and events touching the metadata of a tree node that was created under an already deleted parent threw `RuntimeError: unreachable` ("Parent is not registered"). The parent of such a container is now found from the change that created it. Also, an import rejected by state validation could leave loaded history pointing at containers and values its rollback had removed, so a later export, checkout, or import crashed (#1161). When a change block is present but cannot be parsed (a corrupted snapshot), such a query no longer traps, and later `import`, `export`, `checkout`, `diff`, `revertTo` and `forkAt` calls throw a "cannot parse change block" error instead of working from a partial history. The current state stays readable.
