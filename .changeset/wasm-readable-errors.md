---
"loro-crdt": minor
---

fix: avoid unreadable WASM traps (`RuntimeError: unreachable executed`) on invalid arguments. `getCursor` with an invalid `side`, `getEditorOf` at an out-of-range position, and accessing an unknown container (created by a newer version of loro-crdt) now throw catchable JS errors with readable messages or return `undefined` instead of trapping the WASM instance.

feat: on a Rust panic, the full panic info (message, `file:line`, and the complete stack including the Rust call path) is now stored as a JS `Error` on `globalThis.__LORO_WASM_LAST_PANIC__` before the instance traps, so applications can recover readable diagnostics from a caught `RuntimeError`.

fix: the WASM start hook was never invoked in the `bundler`/`browser`/`base64` builds (the glue checked the JS namespace instead of the wasm exports for `__wbindgen_start`), so no panic hook was installed there at all. The start hook now runs in every build target.

feat: out-of-memory failures are now reported too. Allocation failure in WASM bypasses the panic hook and traps silently; the package now wraps the global allocator so OOM stores `loro-crdt: out of WASM memory: allocation of N bytes failed` on `globalThis.__LORO_WASM_LAST_PANIC__` and prints it to `console.error` before trapping.
