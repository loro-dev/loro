// Copied into nodejs/esm-preload.mjs by post-rollup.ts.
//
// The nodejs CJS glue (loro_wasm.js) locates loro_wasm_bg.wasm with
// `__dirname` + `readFileSync`, which breaks once the entry is bundled into
// ESM output (no `__dirname` in ESM scope). This module runs BEFORE the glue
// (import order in index.mjs) and hands it the wasm bytes resolved from
// `import.meta.url` — which plain node preserves and asset-aware bundlers
// (Vite, Rollup, Webpack) rewrite. Bundlers that leave `new URL` untouched
// (plain esbuild) resolve it relative to the emitted bundle at runtime, so
// copying loro_wasm_bg.wasm next to the bundle is enough.
//
// If the wasm cannot be read here, fall through silently: the glue keeps its
// own `__dirname` fallback for real-CJS module scope and throws a descriptive
// error when neither source works.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

if (globalThis.__LORO_WASM_NODEJS_BYTES__ == null) {
  try {
    globalThis.__LORO_WASM_NODEJS_BYTES__ = readFileSync(
      fileURLToPath(new URL("./loro_wasm_bg.wasm", import.meta.url)),
    );
  } catch {
    // Leave unset — loro_wasm.js falls back to its __dirname resolution.
  }
}
