---
"loro-crdt": patch
---

Make the nodejs entry work under ESM import and ESM bundling. The package now
ships a generated real-ESM wrapper (`nodejs/index.mjs` + `nodejs/esm-preload.mjs`)
that resolves `loro_wasm_bg.wasm` via `import.meta.url` and hands the bytes to
the CJS glue, whose `__dirname`-bound wasm load previously threw a
`ReferenceError` on first touch when the entry was bundled into ESM output.
The `node` condition of the root export and the `./nodejs` subpath now route
`import` to the wrapper while `require` keeps resolving the unchanged CJS entry.
When neither the preloaded bytes nor `__dirname` are available, the glue throws
a descriptive error pointing at the wrapper and the `base64` entry instead of a
bare `ReferenceError`.
