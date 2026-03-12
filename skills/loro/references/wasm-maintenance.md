# WASM Maintenance

## Runtime Model

- Current Loro JS events are synchronous at the JavaScript boundary, but they are still implemented through a pending queue.
- Rust enqueues listener work while internal borrows are active.
- The JavaScript wrapper flushes pending events with `callPendingEvents()` only after the Rust call has returned.
- This keeps callbacks re-entrant from JS without violating Rust borrowing rules.

## Repository Invariant

- Any WASM-exposed API that can enqueue subscription events must flush pending events before control returns to JS.
- In this repository, that means decorating the JS-facing method in `crates/loro-wasm/index.ts` so `callPendingEvents()` runs in `finally`.
- If the wrapper returns a promise, the flush happens in the promise `finally()`.

## Existing Decorated Areas

- `LoroDoc` mutators and barriers such as `commit`, `checkout`, `checkoutToLatest`, `attach`, `detach`, `export`, `import`, `importBatch`, `revertTo`, `diff`, and `applyDiff`
- `EphemeralStoreWasm` mutators such as `set`, `delete`, `apply`, and `removeOutdated`
- `UndoManager.undo()` and `UndoManager.redo()`

## Failure Mode

- If a mutating API enqueues events but is not flushed, the queued microtask path can log:
  - `[LORO_INTERNAL_ERROR] Event not called`

## Mutation Checklist

1. Inspect the Rust method body and its internal calls.
   - Does it commit implicitly?
   - Does it detach or attach state?
   - Does it emit events?
   - Does it apply diffs, imports, or history changes?
2. Check whether the JS name differs from the Rust method name.
   - Keep `#[wasm_bindgen(js_name = "...")]`, TypeScript docs, and wrapper allowlists aligned.
3. Decide whether the method belongs in the decorated allowlist.
   - Add it when it can enqueue subscription callbacks.
   - Skip it when it is a pure read/query API.
4. Check adjacent aliases and wrappers.
   - `importUpdateBatch` is deprecated but forwards to `importBatch`.
   - `exportJsonInIdSpan(...)` has special `subscribePreCommit(...)` semantics and should not introduce an extra implicit commit there.
5. Update or add tests.
   - Favor `crates/loro-wasm/tests/event.test.ts` for flush and event ordering behavior.
   - Favor `crates/loro-wasm/tests/basic.test.ts` for API-level semantics like `revertTo` and `applyDiff`.

## Verification Commands

```sh
pnpm -C crates/loro-wasm build-release
pnpm -C crates/loro-wasm test
```
