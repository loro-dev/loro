---
"loro.js": patch
---

`export({ mode: "snapshot" })` on a detached `loro.js` document now encodes the latest state, version, and frontiers, as Rust and `loro-crdt` do. This includes a document imported lazily from a snapshot that received updates while detached. Before, the export wrote the checked-out state and version next to the full history, so an importer saw an old state as the latest version and a later checkout failed. The exporting document keeps its checkout.
