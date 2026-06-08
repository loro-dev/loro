---
"loro-crdt": patch
---

Fix Node and Vitest bare imports by resolving the package root to the Node.js
WASM entry under the `node` export condition.
