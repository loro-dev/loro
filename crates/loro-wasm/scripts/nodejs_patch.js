// Don't patch this if it already exists (for example in Deno)
if (!globalThis.crypto) {
    // We need this patch because we use `getrandom` crate in Rust, which relies on this patch 
    // for nodejs
    // https://docs.rs/getrandom/latest/getrandom/#nodejs-es-module-support
    // __loroNodeBuiltin is defined near the top of the glue this file is
    // appended to (see patchNodejsGlueForEsm in build.ts); it keeps this
    // require working when the entry is bundled into ESM output.
    const { webcrypto } = __loroNodeBuiltin("crypto");
    Object.defineProperty(globalThis, 'crypto', {
        value: webcrypto,
        writable: true
    });
}
