# Repository Development

Use this chapter when the task is about changing this repository, mapping Loro docs to source, deciding where tests belong, or preserving public API compatibility.

## Source Of Truth Order

- Prefer the current worktree for implementation details, generated bindings, tests, and package versions.
- Use `https://loro.dev/llms-full.txt` as the official docs snapshot when the task asks about user-facing Loro behavior. Search its `# FILE:` markers before loading broad sections.
- When docs and repository disagree, check whether the branch is ahead of published docs. For code changes, make the repository internally consistent first.
- For current JS package behavior, check `crates/loro-wasm/package.json`, `crates/loro-wasm/src/lib.rs`, generated TypeScript declarations inside that file, and `crates/loro-wasm/index.ts`.

## Workspace Map

- `crates/loro`: public Rust API wrappers and integration tests.
- `crates/loro-internal`: core CRDT logic, state, handlers, encoding, diffing, sync, undo, awareness, and internal tests.
- `crates/loro-wasm`: Rust-to-WASM exports, handwritten JS wrapper, package metadata, Vitest/Deno/Bun tests.
- `crates/delta`, `crates/rle`, `crates/kv-store`, `crates/fractional_index`, `crates/loro-common`: shared primitives.
- `docs/`, `crates/loro-internal/docs/`, and `crates/loro-internal/Encoding.md`: encoding and internal design references.
- `examples/` and `crates/examples`: consumer examples and integration fixtures.

## API Lookup Paths

- Public Rust doc/API: start in `crates/loro/src/lib.rs`.
- Internal behavior behind the public wrapper: inspect `crates/loro-internal/src/loro.rs`, `handler.rs`, `state/`, `container/`, `encoding.rs`, `sync.rs`, `event.rs`, and `undo.rs`.
- JS/WASM exports: inspect `crates/loro-wasm/src/lib.rs`; many doc comments feed TypeScript declarations.
- JS runtime behavior and wrapper aliases: inspect `crates/loro-wasm/index.ts`.
- Generated or package-level behavior should be validated against `crates/loro-wasm/tests/*.test.ts`.

## Compatibility Rules

- Treat `loro` and `loro-crdt` as public libraries with downstream users.
- Prefer non-breaking fixes. Add `try_*` methods or new overloads rather than changing existing return types when possible.
- Keep existing panicking methods panicking when compatibility requires it, but use descriptive `expect(...)` messages for internal invariants.
- Invalid external input, malformed bytes, invalid JSON schema, and out-of-bounds user input should return `Err` or JS exceptions.
- Internal corruption or impossible event/diff/import state should fail fast rather than return misleading data or silently skip work.

## Test Placement

- Public Rust API behavior: `crates/loro/tests`, especially `contracts/` for stable surface behavior.
- Internal CRDT behavior: `crates/loro-internal/tests` or module tests near the affected code.
- WASM/TypeScript behavior: `crates/loro-wasm/tests`.
- Encoding/import bugs: prefer small binary or JSON fixtures near the regression test.
- Diff, checkout, import, and state-replay changes may deserve fuzz coverage under `crates/fuzz`; ask before running broad fuzz targets.

## Validation Commands

- Narrow Rust internal check: `cargo check -p loro-internal`.
- Main Rust tests: `pnpm test`.
- Clippy with warnings denied: `pnpm check`.
- WASM package build and tests: `pnpm -C crates/loro-wasm build-release`.
- Loom concurrency tests: `pnpm test-loom`.
- Full WASM release packaging from repo root: `pnpm release-wasm`.

## Change Checklist

1. Identify whether the change affects public Rust API, internal logic, WASM exports, generated JS types, docs, or examples.
2. Update the narrowest source layer first, then propagate wrapper/docs/test changes outward.
3. For `#[wasm_bindgen]` methods that can mutate state, commit implicitly, import/export, checkout, apply diffs, or emit events, read `wasm-maintenance.md`.
4. Add or update focused regression tests near the behavior.
5. Run the narrow validation command first; broaden to `pnpm test`, `pnpm check`, or WASM tests when shared behavior changed.
