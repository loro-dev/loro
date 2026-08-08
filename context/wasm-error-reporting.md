# WASM Error Reporting: Panic Hook, Global Slot, and OOM

Verified against code 2026-08-07.

Why this exists: on `wasm32-unknown-unknown` a Rust panic compiles to the wasm
`unreachable` instruction, so JS only catches an engine-made
`RuntimeError: unreachable executed` with no payload, and the instance is
corrupt afterwards. This document describes the reporting channels built into
`loro-crdt` and the pitfalls around them.

## Channels

- `crates/loro-wasm/src/lib.rs` `init_panic_hook` (installed from the
  `#[wasm_bindgen(start)]` function) reports every panic through two channels:
  a JS `Error` stored on `globalThis.__LORO_WASM_LAST_PANIC__` (`.message` =
  panic message + `file:line:col`, `.stack` = full stack) and `console.error`.
- V8's `Error.stackTraceLimit` defaults to 10 frames, which cuts off exactly
  the Rust call path below the panic machinery. The hook raises the limit to
  100 during capture and restores it afterwards.
- The slot is only initialized when absent (`init_panic_hook`), so the first
  trapped instance's record survives a second loro-crdt copy loading later in
  the same realm.

## `__wbindgen_start` must be called from the wasm exports

`loro_wasm_bg.js` never exports `__wbindgen_start`; it is an export of the wasm
module itself, and the wasm binary has no `start` section. Older glue in
`scripts/bundler_patch.js`, `scripts/browser_patch.js`, and the base64 patch in
`scripts/post-rollup.ts` checked the JS glue namespace for it, so the start
function (and with it any panic hook) silently never ran in the
bundler/browser/base64 builds. The glue now calls `exports.__wbindgen_start()`.
Keep it that way when touching those patches.

## OOM reporting via the global allocator

Allocation failure does not run the panic hook: it goes straight from
`handle_alloc_error` to a trap. The rustc-internal
`__rust_alloc_error_handler*` weak symbols look like the obvious override
point, but the compiler resolves them inside the precompiled std — a
downstream `#[no_mangle]` override links but is never called (verified by
disassembling the artifact with `wasm-tools print`). The stable interception
point is `#[global_allocator]`: `ReportingAlloc` in `src/lib.rs` forwards to
`System` and reports `loro-crdt: out of WASM memory: allocation of N bytes
failed` through the same channel before trapping. It forwards all four
`GlobalAlloc` methods (`realloc`/`alloc_zeroed` included) so dlmalloc's
in-place realloc/calloc fast paths are preserved, with `#[inline(always)]` and
a `#[cold]` report path to keep the overhead to a null check. Only wasm
stack-overflow traps remain unreportable.

## Testing

- `tests/panic_hook.test.ts` triggers a real panic (JSONPath `match()` is
  `unimplemented!()` in loro-internal) and asserts the slot carries the
  message and a deep stack.
- `tests/oom.test.ts` spawns a child `node --wasm-max-mem-pages=1024` process
  (V8 flag caps wasm memory) to force a deterministic OOM, and skips itself if
  the flag ever disappears.
- `tests/panic_repro.test.ts` covers the JS-facing argument-validation errors
  that used to trap.
