# Agent Notes (Loro)

## Invariant: Flush Pending Events In `loro-wasm`

In `crates/loro-wasm/src/lib.rs`, subscription callbacks (`subscribe*`, container `subscribe`, etc.)
do not call user JS immediately. Instead, the binding enqueues JS calls into a global pending queue,
and schedules a microtask check. If the microtask runs before `callPendingEvents()` flushes the
queue, it will log:

- `[LORO_INTERNAL_ERROR] Event not called`

This creates a strict invariant:

- **Any WASM-exposed API that can enqueue subscription events must flush pending events before
  returning control back to JS.**

To avoid adding overhead to every single op, we only wrap (decorate) a small allowlist of
methods on the JS side. The wrapper calls `callPendingEvents()` in a `finally` block.

### How To Maintain

- When adding or changing a `#[wasm_bindgen]` API in `crates/loro-wasm/src/lib.rs` that can
  *mutate document state*:
  - If it can trigger an implicit commit / barrier (`commit`, `with_barrier` /
    `implicit_commit_then_stop`), emit events (`emit_events`), or applies diffs (e.g. `revertTo`,
    `applyDiff`), it typically **must** flush pending events.
  - Add its JS name to the allowlist in `crates/loro-wasm/index.ts` near the bottom:
    `decorateMethods(LoroDoc.prototype, [...])` (or the relevant prototype allowlist).
  - If it is a pure read/query API (no state mutation, no commit/barrier, no event emission),
    do **not** decorate it, to avoid unnecessary per-call cost.

### Quick Check

With active subscriptions (`doc.subscribe(...)` / container `subscribe(...)`), calling mutating APIs
should not produce the error above. A recommended local check is:

```sh
pnpm -C crates/loro-wasm build-release
```
