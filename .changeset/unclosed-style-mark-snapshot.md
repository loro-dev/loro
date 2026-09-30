---
"loro-crdt": patch
---

Fix a panic (a `RuntimeError: unreachable` trap in WASM) when reading a document forked or snapshotted at a version that holds a mark's start but not its end ("unclosed style mark"). Such a start anchor is now decoded like `checkout` keeps it: it styles nothing until its end arrives. A shallow snapshot of a document whose history ends at such a start no longer fails with `FrontiersNotFound`.
