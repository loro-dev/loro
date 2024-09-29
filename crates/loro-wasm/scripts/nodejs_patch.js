// Don't patch this if it already exists (for example in Deno)
if (!globalThis.crypto) {
    // We need this patch because we use `getrandom` crate in Rust, which relies on this patch 
    // for nodejs
    // https://docs.rs/getrandom/latest/getrandom/#nodejs-es-module-support
    const { webcrypto } = require("crypto");
    Object.defineProperty(globalThis, 'crypto', {
        value: webcrypto,
        writable: true
    });
}
