# Repository Guidelines

## Project Snapshot

This repository is a Rust workspace for the Loro CRDT library, with JS/WASM,
TypeScript, and MoonBit packaging around the Rust core.

- `crates/loro`: public Rust API. Treat this as a stable downstream-facing crate.
- `crates/loro-internal`: core CRDT implementation, including oplog, state,
  diff calculation, encoding, containers, DAG/version logic, and checkout/import
  behavior.
- `crates/loro-wasm`: `loro-crdt` WASM/TypeScript package. Read its nested
  `AGENTS.md` before changing WASM bindings, package exports, or JS wrappers.
- `crates/delta`, `crates/rle`, `crates/kv-store`, `crates/fractional_index`,
  and `crates/loro-common`: shared primitives used by the core crates.
- `packages/fractional-index`: TypeScript package for the fractional index
  algorithm.
- `examples/` and `crates/examples`: integration examples and bundler smoke
  tests.
- `moon/`: MoonBit implementation of the Loro binary codec. Use the MoonBit
  skill in `skills/moonbit` when working there.
- `skills/loro`: project skill for user-facing Loro guidance. Prefer loading
  its focused reference files over copying broad CRDT background into answers.

## Build, Test, And Development Commands

Use narrow commands first, then broaden when touching shared behavior.

- Install JS dependencies when needed: `pnpm install --frozen-lockfile`.
- Build Rust workspace: `cargo build`.
- Fast internal check: `cargo check -p loro-internal`.
- Rust format: `cargo fmt --all`.
- Rust lint: `pnpm check` (`cargo clippy --all-features -- -Dwarnings`).
- Main Rust tests: `pnpm test` (`cargo nextest run --features=test_utils,jsonpath --no-fail-fast && cargo test --doc`).
- Internal doctests: `cargo test -p loro-internal --doc`.
- Loom concurrency test: `pnpm test-loom`.
- WASM package build/test: `pnpm release-wasm`.
- WASM local dev build: `pnpm -C crates/loro-wasm build-dev`.
- Bundler smoke tests after WASM packaging or entrypoint changes:
  `pnpm test-bundlers`, and for browser runtime coverage
  `pnpm --dir examples/bundler-smoke-tests run test:browser`.
- Fractional-index TS package: `pnpm test-fractional-index`.
- Short fuzz corpus smoke: `pnpm run-fuzz-corpus`.
- MoonBit codec, when `moon` is available: run commands from `moon/`, usually
  `moon check`, `moon test`, and `moon fmt`.

Do not run broad fuzzing or long browser matrices without checking with the user
when time/cost is unclear.

## Testing Guidelines

Add regression tests near the behavior being fixed.

- Public Rust API tests: `crates/loro/tests`.
- Internal behavior tests: `crates/loro-internal/tests` or local module tests.
- WASM behavior tests: `crates/loro-wasm/tests`.
- MoonBit codec tests: `moon/loro_codec/*_test.mbt` plus Rust/Moon e2e drivers
  documented in `docs/moon-codec-fuzzing.md`.
- Import, encoding, and replay bugs should use small binary or JSON fixtures
  when possible.
- Changes touching internal diff calculation, checkout, import, state replay, or
  encoding may need fuzz coverage under `crates/fuzz`; ask before running the
  broad `cargo +nightly fuzz run all` style targets.

## Coding Style And Boundaries

Use standard Rust formatting with `rustfmt`. Keep imports and chained calls
formatted by the tool. Rust functions/modules use `snake_case`; Rust types use
`CamelCase`. JS/TS bindings in `loro-wasm` must preserve established exported API
names used by tests and docs.

Prefer existing crate-local helpers and data structures over new abstractions.
Keep changes scoped to the relevant crate boundary. Do not refactor shared CRDT
machinery while fixing unrelated package, docs, or binding issues.

## Public API Compatibility

The `loro` crate and `loro-crdt` package are public libraries with downstream
users. Avoid breaking changes unless there is no safe alternative.

- Prefer adding `try_*` methods returning `Option` or `Result` over changing an
  existing method signature.
- If an existing public method must keep panicking for compatibility, prefer a
  descriptive `expect()` message over an opaque `unwrap()`.
- Only change public return types or names when required by a critical
  correctness or safety issue.
- Add a changeset for publishing behavior or package output changes.

## Internal Invariants

Internal corruption should fail fast. Invalid external input should return an
error.

- Do not let the system continue after a violated internal invariant, such as a
  missing state that should exist, an impossible event shape, or a diff that
  cannot be composed.
- Do not silently skip data, return defaults, or report success when internal
  state is known to be inconsistent.
- Malformed user input, invalid JSON schema, decode failures, and out-of-bounds
  external requests should return `Err` where the API supports it.
- Returning wrong data is worse than panicking on corrupted internal state.

## WASM Event Flush Invariant

In `crates/loro-wasm/src/lib.rs`, subscription callbacks enqueue JS calls into a
global pending queue instead of calling user JS immediately. If the microtask
check runs before `callPendingEvents()` flushes that queue, it logs:

```text
[LORO_INTERNAL_ERROR] Event not called
```

Any WASM-exposed API that can enqueue subscription events must flush pending
events before returning to JS. The JS-side allowlist lives near the bottom of
`crates/loro-wasm/index.ts` in `decorateMethods(...)`. When adding or changing a
`#[wasm_bindgen]` API that can mutate document state, trigger implicit commits
or barriers, emit events, or apply diffs, update the relevant allowlist. Pure
read/query APIs should not be decorated. See `crates/loro-wasm/AGENTS.md` before
editing this area.

## Release And Generated Files

- WASM release output and versions are synchronized through
  `scripts/sync-loro-version.ts` and the `pnpm release-wasm` / changesets flow.
- Rust crate releases use `scripts/cargo-release.ts` and `cargo-release`; keep
  version bumps focused.
- Do not hand-edit generated package output from the WASM build. Regenerate it
  with the package scripts.
- Keep lockfiles and small fixtures when they are intentionally affected by the
  change. Do not churn them for unrelated work.

## Agent Workflow

- Start with `git status --short --branch` and treat uncommitted changes as user
  work unless you made them in the current turn.
- Read the nearest `AGENTS.md` before editing a subtree.
- Use `rg` / `rg --files` for search and repository mapping.
- Load `skills/loro` for user-facing Loro usage, CRDT modeling, sync,
  persistence, editor integration, or performance guidance. Load
  `skills/moonbit` for work under `moon/`.
- Make the smallest durable context or code change that solves the request.
- Validate with the narrowest meaningful command first and report any broader
  checks not run.

## Commit And PR Notes

History uses short imperative commits, often prefixed by scope such as `fix:`,
`test:`, `chore:`, or `refactor:`. Keep commits focused and include fixtures or
tests with fixes. PRs should describe what changed, why, validation commands, and
linked issues or production traces when relevant.
