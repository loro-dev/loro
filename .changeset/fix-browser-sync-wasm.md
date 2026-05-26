---
"loro-crdt": patch
---

Fix the browser WASM loader for remapped bundler builds. The browser entry now avoids setting `XMLHttpRequest.responseType` on synchronous document requests, which browsers reject, reads the WASM bytes through a one-byte text decoding path, and emits explicit WASM re-exports so Parcel scope-hoisted builds can run in the browser.
