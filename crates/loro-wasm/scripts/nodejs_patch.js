const { webcrypto } = require("crypto");
Object.defineProperty(globalThis, 'crypto', {
    value: webcrypto,
    writable: true
});