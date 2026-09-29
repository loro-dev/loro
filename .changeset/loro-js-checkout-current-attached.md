---
"loro.js": patch
---

`checkout(frontiers)` on a `loro.js` document now behaves like `loro-crdt` when `frontiers` is the current version: nothing changes, and the document stays (or becomes) attached when that version is the latest one. Before, `doc.checkout(doc.frontiers())` detached the document, so later imports were not applied to its state. Checking out the latest version from an older one still leaves the document detached, as in Rust.
