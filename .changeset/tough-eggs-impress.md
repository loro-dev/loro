---
"loro-crdt": patch
---

Fix WASI builds by using native calls instead of js-only wasm32 bindings (`Date.now`, `getrandom`)
