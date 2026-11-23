---
"loro-crdt": patch
---

Keep a tree's `enableFractionalIndex(jitter)` / `disableFractionalIndex()` setting when a snapshot is imported into the document and in `fork()` / `forkAt()`. Snapshot import used to reset it to jitter 0, while update import kept it (#1288).
