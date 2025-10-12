// See https://github.com/loro-dev/loro/issues/440
// Without this patch, Cloudflare Worker would raise issue like: "Uncaught TypeError: wasm2.__wbindgen_start is not a function"
import * as wasm from "./loro_wasm_bg.wasm";
import * as imports from "./loro_wasm_bg.js";

if (wasm.__wbindgen_start) {
    imports.__wbg_set_wasm(wasm);
    // Seed wasm-bindgen's externref table so constants like `true`/`false`
    // don't alias arbitrary externrefs when we instantiate manually.
    if (typeof imports.__wbindgen_init_externref_table === "function") {
        imports.__wbindgen_init_externref_table();
    }
    wasm.__wbindgen_start();
} else if ('Bun' in globalThis) {
    const bytes = await Bun.file(wasm.default).bytes();
    const wasmModule = new WebAssembly.Module(bytes);
    const instance = new WebAssembly.Instance(wasmModule, {
        "./loro_wasm_bg.js": imports,
    });
    imports.__wbg_set_wasm(instance.exports);
    // Bun path needs the same externref initialisation.
    if (typeof imports.__wbindgen_init_externref_table === "function") {
        imports.__wbindgen_init_externref_table();
    }
} else {
    const wkmod = await import("./loro_wasm_bg.wasm");
    const instance = new WebAssembly.Instance(wkmod.default, {
        "./loro_wasm_bg.js": imports,
    });
    imports.__wbg_set_wasm(instance.exports);
    if (typeof imports.__wbindgen_init_externref_table === "function") {
        imports.__wbindgen_init_externref_table();
    }
}
export * from "./loro_wasm_bg.js";
