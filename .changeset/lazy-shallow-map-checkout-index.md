---
"loro-crdt": patch
---

Speed up the first import that is concurrent with a local edit to the same map on a shallow doc. The checkout index no longer decodes every map in the shallow root up front; each map's shallow-root entries are added the first time a diff needs that map. On a doc with 100k maps in the shallow root this import went from ~240 ms to ~3.5 ms natively, and from ~2.8 s to ~6 ms in the WASM build.
