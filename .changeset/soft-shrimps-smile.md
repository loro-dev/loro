---
"loro-crdt": patch
"loro-crdt-map": patch
---

Optimize snapshot export for shallow documents by reusing cached shallow-root state instead of checking out to the shallow root and back to latest.
