---
"loro-crdt": patch
---

Re-exporting a shallow document at its own shallow root (a shallow snapshot at the same root, or a full snapshot of a shallow doc) now drops map/list containers that were deleted before the root. Shallow snapshots from older versions could still carry such containers, and re-exporting them used to keep that deleted content; it is now filtered with the same rules as a fresh shallow export, while tree node metadata that can still be revived is kept.

If a shallow document's stored root state is internally inconsistent (a container's recorded parent does not match the container that references it) and some container looks unreachable, nothing is dropped: the root state is exported as it is, like before, so a container that is still referenced is never lost.
