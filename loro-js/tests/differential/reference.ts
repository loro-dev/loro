import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

import * as loroJs from "../../src/index";

import type { EngineModule } from "./engine";

/**
 * Loads the Rust implementation through the workspace `loro-crdt` Node build.
 *
 * Build it with `pnpm --dir loro-js build:reference` (a dev WASM build of
 * `crates/loro-wasm`), or point `LORO_JS_REFERENCE` at another `nodejs/index.js`
 * build, for example one made from an unmerged Rust branch.
 */
export function loadReference(): EngineModule {
  const path =
    process.env["LORO_JS_REFERENCE"] ??
    fileURLToPath(new URL("../../../crates/loro-wasm/nodejs/index.js", import.meta.url));
  if (!existsSync(path)) {
    throw new Error(
      `Rust reference build not found at ${path}. ` +
        "Run `pnpm --dir loro-js build:reference` first.",
    );
  }
  const require = createRequire(import.meta.url);
  return require(path) as EngineModule;
}

export function loadLoroJs(): EngineModule {
  return loroJs as unknown as EngineModule;
}
