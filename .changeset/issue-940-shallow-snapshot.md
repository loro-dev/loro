---
"loro-crdt": patch
"loro-crdt-map": patch
---

Reduce memory spikes when exporting snapshots from shallow documents.

When a shallow document is re-exported from its existing shallow root with only a small tail of updates, Loro now reuses the stored shallow-root state instead of decoding all containers just to re-encode the same state.
