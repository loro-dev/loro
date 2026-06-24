/* __LORO_BROWSER_PATCH_IMPORTS__ */

// Keep the import object as a plain object built from named imports. Parcel
// scope hoisting can lose a direct namespace import when it is used as a
// WebAssembly import object.
const imports = {
  /* __LORO_BROWSER_PATCH_IMPORT_OBJECT__ */
};
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
  // A document cannot set `responseType` on synchronous XHR. Force a one-byte
  // text decoding instead and convert the result back to wasm bytes.
  request.overrideMimeType("text/plain; charset=x-user-defined");
  request.send(null);

  if (request.status !== 0 && (request.status < 200 || request.status >= 300)) {
    throw new Error(
      `Failed to load loro-crdt WASM from ${url}: ${request.status} ${request.statusText}`,
    );
  }

  const text = request.responseText;
  const bytes = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i++) {
    bytes[i] = text.charCodeAt(i) & 0xff;
  }

  return bytes;
}

function instantiateSync(bytes, importObject) {
  const module = new WebAssembly.Module(bytes);
  return new WebAssembly.Instance(module, importObject);
}

const wasmUrl = new URL("./loro_wasm_bg.wasm", import.meta.url);
const instance = instantiateSync(loadWasmBytesSync(wasmUrl.href), WASM_IMPORTS);

finalize(instance.exports);

/* __LORO_BROWSER_PATCH_EXPORTS__ */
