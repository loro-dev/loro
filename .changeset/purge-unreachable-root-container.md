---
"loro-crdt": patch
---

`deleteRootContainer` now empties a root container that is already unreachable,
such as a mergeable root whose owning tree node was deleted, together with every
container nested in it (child containers, tree node metadata and mergeable
children, also a mergeable child whose map key was later overwritten or deleted).
Their content no longer stays in the document state, in state-only exports, or
in shallow snapshots exported at a frontier after the purge. The old child
container of a normal (non-mergeable) map value that was overwritten is not
reached; its content can remain in state-only exports.

Potentially breaking: `deleteRootContainer` now throws for a non-root container,
for a container the document does not have (such as an unknown mergeable
container), when emptying the container fails, and on a detached document. It
used to ignore these silently.
