import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

/**
 * Loads the Rust implementation (`loro-crdt`, nodejs WASM target) as a
 * differential-testing reference. Build it with
 * `pnpm -C crates/loro-wasm build-dev` (or `build-release`), or point
 * `LORO_WASM_NODEJS` at another build's `nodejs/index.js`.
 *
 * Returns `undefined` when no build is available so the differential suites can
 * skip instead of failing on machines without a WASM toolchain.
 */
export function loadRustReference(): RustReference | undefined {
  const path =
    process.env.LORO_WASM_NODEJS ??
    fileURLToPath(new URL("../../../crates/loro-wasm/nodejs/index.js", import.meta.url));
  if (!existsSync(path)) {
    if (process.env.LORO_REQUIRE_WASM_REFERENCE === "1") {
      throw new Error(`Rust WASM reference build is missing: ${path}`);
    }
    return undefined;
  }
  return createRequire(import.meta.url)(path) as RustReference;
}

export type RustReference = typeof import("../../src/index");
