import * as rawWasm from './loro_wasm_bg.wasm';
import * as imports from './loro_wasm_bg.js';

// Normalize how bundlers expose the wasm module/exports.
const toModuleOrExports = (wasm) => {
  if (!wasm) return wasm;
  if (wasm instanceof WebAssembly.Module) return wasm;
  if (typeof wasm === 'object' && 'default' in wasm) {
    return wasm.default ?? wasm;
  }
  // rsbuild doesn't provide a default export when importing wasm.
  return wasm;
};

const wasmModuleOrExports = toModuleOrExports(rawWasm);

// Helper: ensure we end up with exports + optionally run externref init.
const finalize = (exports) => {
  imports.__wbg_set_wasm(exports);
  if (typeof imports.__wbindgen_init_externref_table === 'function') {
    imports.__wbindgen_init_externref_table();
  }
};

if (wasmModuleOrExports && wasmModuleOrExports.__wbindgen_start) {
  // See https://github.com/loro-dev/loro/issues/440
  // Without this patch, Cloudflare Worker would raise issue like: "Uncaught TypeError: wasm2.__wbindgen_start is not a function"
  // Already the initialized exports object (Cloudflare Workers path).
  finalize(wasmModuleOrExports);
  wasmModuleOrExports.__wbindgen_start();
} else if ('Bun' in globalThis) {
  // Bun's wasm runtime (1.3.0 as of Oct 2025) sometimes reads externref slot 1
  // (reserved for booleans by wasm-bindgen) as the global object, causing APIs
  // like `LoroText.toDelta()` to return cyclic structures. Re-running the
  // wasm-bindgen externref table initializer after instantiation resets the
  // table so booleans stay primitives and avoids the infinite recursion seen in
  // Bun tests during `pnpm release-wasm`.
  let instance;
  if (wasmModuleOrExports instanceof WebAssembly.Module) {
    ({ instance } = await WebAssembly.instantiate(wasmModuleOrExports, {
      './loro_wasm_bg.js': imports,
    }));
  } else {
    const url = Bun.pathToFileURL(wasmModuleOrExports);
    ({ instance } = await WebAssembly.instantiateStreaming(fetch(url), {
      './loro_wasm_bg.js': imports,
    }));
  }
  finalize(instance.exports);
} else {
  // Browser/node-like bundlers: either we already have exports, or a Module/URL.
  const wkmod =
    wasmModuleOrExports instanceof WebAssembly.Module
      ? wasmModuleOrExports
      : await import('./loro_wasm_bg.wasm');
  const module =
    wkmod instanceof WebAssembly.Module
      ? wkmod
      : (wkmod && wkmod.default) || wkmod;
  const instance =
    module instanceof WebAssembly.Instance
      ? module
      : new WebAssembly.Instance(module, {
        './loro_wasm_bg.js': imports,
      });
  finalize(instance.exports ?? instance);
}

export * from './loro_wasm_bg.js';
