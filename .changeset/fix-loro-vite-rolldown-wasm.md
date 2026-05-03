---
"loro-crdt": patch
"loro-crdt-map": patch
---

Add a browser package remapping so Vite/Rolldown production builds load WASM without top-level await or circular wasm wrapper chunks.

Also make the base64 entry easier to bundle with plain esbuild, Rollup, and Next.js Webpack by avoiding static Node builtin `require()` calls and top-level await in browser bundles.
