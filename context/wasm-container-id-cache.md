# WASM container id cache

Verified against code 2026-09-04.

`LoroMap`, `LoroList`, `LoroText`, `LoroTree`, `LoroMovableList`, and
`LoroCounter` expose `id` from Rust through wasm-bindgen. The generated getter
crosses into WASM and creates a new JS string on every call. The same holds for
`kind()`, which returns a constant string per container class.

## Identity and lifetime

- `HandlerTrait::id` in `crates/loro-internal/src/handler.rs` reads the immutable
  `BasicHandler.id` for an attached handler. Detached handlers return their
  type-specific `ID::NONE_ID` placeholder.
- Attaching does not rebind the detached JS wrapper. `insertContainer` returns a
  new attached wrapper, while the original detached handler records an attached
  back-reference used by `getAttached()`.
- Calling `getAttached()` on an already attached wrapper clones the Rust handler
  into another JS wrapper. The two wrappers have the same container id but
  separate wasm-bindgen pointers and cache entries.
- wasm-bindgen sets `__wbg_ptr` to zero in `__destroy_into_raw()`. An `id` read
  after `free()` must keep raising wasm-bindgen's null-pointer error.

`scripts/container_id_cache_patch.js` therefore stores the id in a
non-enumerable, module-private Symbol property on the JS wrapper. It clears the
property during `__destroy_into_raw()` and bypasses the cache for a zero
pointer. The cache has exactly the wrapper's lifetime and does not retain the
wrapper from another object.

`kind()` is memoized once per container class (the value is class-constant, so
no per-wrapper entry is needed). Both the id cache and the kind memo bypass the
cache for a zero `__wbg_ptr`, so reads after `free()` keep raising
wasm-bindgen's null-pointer error. Rust-side `kind()` reads
(`js_to_container` in `src/convert.rs`) go through the same patched prototype
method and share the memo.

## Package targets and checks

`scripts/build.ts` appends the cache patch to the raw wasm-bindgen module for
each of `nodejs`, `web`, `browser`, and `bundler`. Normal package entrypoints
and exported raw-binding subpaths therefore use the same decorated classes;
`post-rollup.ts` derives `base64` from the patched bundler target. Do not patch
generated `loro_wasm*.js` files by hand.

Run `pnpm bench-wasm-container-id` from the repository root after a dev or
release WASM build. The benchmark separates repeated reads of one wrapper,
first reads of many wrappers, a mirror-like reused-wrapper pass, and a
fresh-wrapper boundary.
