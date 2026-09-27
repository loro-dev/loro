---
"loro.js": patch
---

`export({ mode: "snapshot" })` on a detached `loro.js` document now encodes the latest state, as Rust and `loro-crdt` do. Before, it wrote the checked-out state and version next to the full history. The resulting snapshot showed an old state as the latest version, and checking out on the importing side failed with "snapshot version does not match its history". The exporting document keeps its checkout.
