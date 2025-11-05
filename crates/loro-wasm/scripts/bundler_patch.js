// See https://github.com/loro-dev/loro/issues/440
// Without this patch, Cloudflare Worker would raise issue like: "Uncaught TypeError: wasm2.__wbindgen_start is not a function"
import * as wasm from "./loro_wasm_bg.wasm";
import * as imports from "./loro_wasm_bg.js";

if (wasm.__wbindgen_start) {
  imports.__wbg_set_wasm(wasm);
  wasm.__wbindgen_start();
} else if ("Bun" in globalThis) {
  const { instance } = await WebAssembly.instantiateStreaming(
    fetch(Bun.pathToFileURL(wasm.default)),
    {
      "./loro_wasm_bg.js": imports,
    },
  );
  imports.__wbg_set_wasm(instance.exports);

  // Bun's wasm runtime (1.3.0 as of Oct 2025) sometimes reads externref slot 1
  // (reserved for booleans by wasm-bindgen) as the global object, causing APIs
  // like `LoroText.toDelta()` to return cyclic structures. Re-running the
  // wasm-bindgen externref table initializer after instantiation resets the
  // table so booleans stay primitives and avoids the infinite recursion seen in
  // Bun tests during `pnpm release-wasm`.
  if (typeof imports.__wbindgen_init_externref_table === "function") {
    imports.__wbindgen_init_externref_table();
  }
} else {
  const wkmod = await import("./loro_wasm_bg.wasm");
  const instance = new WebAssembly.Instance(wkmod.default, {
    "./loro_wasm_bg.js": imports,
  });
  imports.__wbg_set_wasm(instance.exports);
}
export * from "./loro_wasm_bg.js";
