---
"loro-crdt": patch
---

Re-exporting a shallow document at its own shallow root (a shallow snapshot at the same root, or a full snapshot of a shallow doc) now drops map/list containers that were deleted before the root. Shallow snapshots from older versions could still carry such containers, and re-exporting them used to keep that deleted content; it is now filtered with the same rules as a fresh shallow export, while tree node metadata that can still be revived is kept.
