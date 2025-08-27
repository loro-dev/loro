// See https://github.com/loro-dev/loro/issues/440
// Without this patch, Cloudflare Worker would raise issue like: "Uncaught TypeError: wasm2.__wbindgen_start is not a function"
import * as wasm from "./loro_wasm_bg.wasm";
import * as imports from "./loro_wasm_bg.js";

if (wasm.__wbindgen_start) {
    imports.__wbg_set_wasm(wasm);
    wasm.__wbindgen_start();
} else if (!('Bun' in globalThis)) {
    const wkmod = await import("./loro_wasm_bg.wasm");
    const instance = new WebAssembly.Instance(wkmod.default, {
        "./loro_wasm_bg.js": imports,
    });
    imports.__wbg_set_wasm(instance.exports);
}
export * from "./loro_wasm_bg.js";
