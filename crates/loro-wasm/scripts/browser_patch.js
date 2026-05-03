import * as imports from "./loro_wasm_bg.js";

const WASM_IMPORTS = {
  "./loro_wasm_bg.js": imports,
};

const finalize = (exports) => {
  imports.__wbg_set_wasm(exports);
  tryStart(imports);
};

function tryStart(imports) {
  if (typeof imports.__wbindgen_start === "function") {
    imports.__wbindgen_start();
  }
}

// Keep this entry synchronous without top-level await. Vite/Rolldown can
// otherwise create circular wasm wrapper chunks in production builds.
function loadWasmBytesSync(url) {
  if (typeof XMLHttpRequest !== "function") {
    throw new Error(
      "loro-crdt browser build requires XMLHttpRequest for synchronous WASM loading. Use the nodejs, web, base64, or bundler entry for this runtime.",
    );
  }

  const request = new XMLHttpRequest();
  request.open("GET", url, false);
  request.responseType = "arraybuffer";
  request.send(null);

  if (request.status !== 0 && (request.status < 200 || request.status >= 300)) {
    throw new Error(
      `Failed to load loro-crdt WASM from ${url}: ${request.status} ${request.statusText}`,
    );
  }

  if (!(request.response instanceof ArrayBuffer)) {
    throw new Error(
      "Failed to load loro-crdt WASM: response is not an ArrayBuffer",
    );
  }

  return request.response;
}

function instantiateSync(bytes, importObject) {
  const module = new WebAssembly.Module(bytes);
  return new WebAssembly.Instance(module, importObject);
}

const wasmUrl = new URL("./loro_wasm_bg.wasm", import.meta.url);
const instance = instantiateSync(loadWasmBytesSync(wasmUrl.href), WASM_IMPORTS);

finalize(instance.exports);

export * from "./loro_wasm_bg.js";
