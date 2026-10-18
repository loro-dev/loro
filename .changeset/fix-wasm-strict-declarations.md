---
"loro-crdt": patch
---

Fix TypeScript declaration errors when consumers use `skipLibCheck: false` and `strict: true` across the bundler, nodejs, web, browser, and base64 entries. Correct the key type of `toJsonWithReplacer`, the return type of `EphemeralStoreWasm.removeOutdated`, and the callback parameter of `LoroText.iter` to match their existing runtime behavior.
