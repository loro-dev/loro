import * as loroJs from "../../src/index";
import { loadRustReference } from "../support/rust-reference";

import type { EngineModule } from "./engine";

/**
 * The Rust implementation from the shared differential reference
 * (`tests/support/rust-reference.ts`: `crates/loro-wasm/nodejs`, or
 * `LORO_WASM_NODEJS`). Undefined when that build is missing, so the suite
 * skips; `LORO_REQUIRE_WASM_REFERENCE=1` makes a missing build an error.
 */
export function loadReference(): EngineModule | undefined {
  return loadRustReference() as unknown as EngineModule | undefined;
}

export function loadLoroJs(): EngineModule {
  return loroJs as unknown as EngineModule;
}
