# WASM Package Guidelines

This subtree builds the `loro-crdt` JS/WASM package. It contains the Rust
`#[wasm_bindgen]` bindings in `src/lib.rs`, the JS/TS wrapper in `index.ts`,
Rollup/build scripts, package exports, and WASM-specific tests.

## Commands

Run commands from the repository root unless noted.

- Build/test release package: `pnpm release-wasm`.
- Local dev package build: `pnpm -C crates/loro-wasm build-dev`.
- Package-local release build: `pnpm -C crates/loro-wasm build-release`.
- Package-local tests after an existing build: `pnpm -C crates/loro-wasm test`.
- Fast bundler smoke tests after entrypoint/export changes:
  `pnpm test-bundlers`.
- Browser runtime smoke tests for packaging changes:
  `pnpm --dir examples/bundler-smoke-tests run test:browser`.

`pnpm release-wasm` runs version sync, installs this package's dependencies, and
builds the release artifacts. Use it for final validation when changing
`src/lib.rs`, `index.ts`, package exports, Rollup config, or build scripts.

## Pending Events Invariant

Subscription callbacks (`subscribe*`, container `subscribe`, and related APIs)
do not call user JS immediately. Rust queues JS calls into a global pending queue
and schedules a microtask check. If the microtask runs before
`callPendingEvents()` flushes the queue, the package logs:

```text
[LORO_INTERNAL_ERROR] Event not called
```

Any WASM-exposed API that can enqueue subscription events must flush pending
events before returning control to JS. This is intentionally implemented as a
small JS-side allowlist in `index.ts` rather than wrapping every method.

When adding or changing a `#[wasm_bindgen]` API in `src/lib.rs`, check whether it
can:

- mutate document or container state,
- trigger an implicit commit or barrier (`commit`, `with_barrier`,
  `implicit_commit_then_stop`),
- emit events,
- apply diffs (`revertTo`, `applyDiff`), or
- change ephemeral store state that has JS subscribers.

If yes, add the JS method name to the relevant installed `decorateMethods(...)`
allowlist near the bottom of `index.ts`. Today those wrappers cover
`LoroDoc.prototype`, `EphemeralStoreWasm.prototype`, and `UndoManager.prototype`;
add another prototype only when the wrapper is wired there. Pure read/query APIs
should not be decorated.

A quick behavioral check is to run with an active `doc.subscribe(...)` or
container `subscribe(...)` and confirm the mutation does not produce the internal
error above. Keep or add a regression test when the issue is observable from JS.

## Packaging Rules

- Preserve the public `loro-crdt` API names and package export paths used by
  tests and docs.
- Do not hand-edit generated package output. Regenerate with `build-dev`,
  `build-release`, or `pnpm release-wasm`.
- Package entrypoint changes must consider `bundler`, `browser`, `nodejs`,
  `web`, and `base64` outputs.
- Vite and Webpack can emit the `.wasm` asset from `new URL(...)`; plain esbuild
  and Rollup need either the `base64` entry or an explicit asset copy. Keep the
  bundler smoke tests aligned with these expectations.
- If package output or published behavior changes, add a changeset.
