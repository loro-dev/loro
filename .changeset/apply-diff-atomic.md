---
"loro-crdt": patch
---

`applyDiff` and `revertTo` are now all or nothing: when they throw (for example on an out-of-range position, a diff made for another version of the document, or a diff type that doesn't match its container), the document, its history and its subscribers are left as they were. Uncommitted edits made before the call are kept, still uncommitted. Applying a diff that names a tree parent the document doesn't have now throws instead of trapping.
