# Repository Guidelines

## Project Structure & Module Organization

This is a Rust workspace with JS/WASM packaging around the core CRDT library.
Key crates live under `crates/`: `loro` is the public Rust API, `loro-internal`
contains core CRDT logic, `loro-wasm` exposes the WASM/TypeScript package, and
`delta`, `rle`, `kv-store`, and `fractional_index` hold shared primitives.
Integration and regression tests are mostly in `crates/loro/tests` and
`crates/loro-internal/tests`; WASM tests and package files are in
`crates/loro-wasm`. Examples live in `examples/` and `crates/examples`.

## Build, Test, and Development Commands

- `cargo build`: build the Rust workspace.
- `cargo check -p loro-internal`: quickly validate core internals.
- `cargo test -p loro-internal --doc`: run Rust doctests for internal APIs.
- `pnpm test`: run the main Rust test suite via nextest plus doctests.
- `pnpm check`: run clippy with all features and deny warnings.
- `pnpm release-wasm`: sync versions and build the release WASM package.
- `pnpm test-loom`: run loom concurrency tests for `crates/loro/tests/multi_thread_test.rs`.

## Coding Style & Naming Conventions

Use standard Rust formatting with `rustfmt`; keep imports and chained calls formatted
by the tool. Prefer explicit, small APIs and existing crate-local helpers over new
abstractions. Rust items use `snake_case` for functions/modules and `CamelCase` for
types. JS/TS bindings in `loro-wasm` should preserve the established exported API
names used by tests and docs.

## Testing Guidelines

Add regression tests near the behavior being fixed: Rust API tests in
`crates/loro/tests`, internal tests in `crates/loro-internal/tests` or module tests,
and WASM behavior in `crates/loro-wasm/tests`. For import/encoding bugs, prefer
fixture-based tests with small binary fixtures. Run the narrow package test first,
then `pnpm test` when the change affects shared behavior. For changes touching
internal diff calculation, checkout, import, or state-replay logic, also consider
the fuzz targets in `crates/fuzz`; ask whether to run the broader `fuzz all`
target before spending the extra time.

## Commit & Pull Request Guidelines

History uses short imperative commits, often prefixed by scope such as `fix:`,
`test:`, `chore:`, or `refactor:`. Keep commits focused and include fixtures or
tests with fixes. PRs should describe what changed, why, validation commands, and
linked issues or production traces when relevant. Add a changeset when publishing
behavior or package output changes.

## Agent-Specific Notes

### Invariant: Flush Pending Events In `loro-wasm`

In `crates/loro-wasm/src/lib.rs`, subscription callbacks (`subscribe*`,
container `subscribe`, etc.) do not call user JS immediately. The binding
enqueues JS calls into a global pending queue and schedules a microtask check.
If the microtask runs before `callPendingEvents()` flushes the queue, it logs:

- `[LORO_INTERNAL_ERROR] Event not called`

Any WASM-exposed API that can enqueue subscription events must flush pending
events before returning control to JS. To avoid adding overhead to every op, only
a small JS-side allowlist is wrapped; the wrapper calls `callPendingEvents()` in
a `finally` block.

When adding or changing a `#[wasm_bindgen]` API in `crates/loro-wasm/src/lib.rs`
that can mutate document state, check whether it can trigger an implicit commit
or barrier (`commit`, `with_barrier`, `implicit_commit_then_stop`), emit events
(`emit_events`), or apply diffs (`revertTo`, `applyDiff`). If so, add its JS
name to the allowlist near the bottom of `crates/loro-wasm/index.ts`:
`decorateMethods(LoroDoc.prototype, [...])` or the relevant prototype allowlist.
Pure read/query APIs should not be decorated.

Quick check with active subscriptions (`doc.subscribe(...)` or container
`subscribe(...)`): mutating APIs should not produce the error above. A useful
local check is:

```sh
pnpm -C crates/loro-wasm build-release
```
