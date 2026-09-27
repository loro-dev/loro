# loro.js

Pure TypeScript implementation of Loro (`loro-js/src`). It must read and write
the same binary and JSON data as Rust (`loro-crdt`) and give the same results.

## Context

- Index structures, complexity contracts, and benchmarks:
  [context/loro-js-performance.md](../context/loro-js-performance.md).
- Randomized Text comparison with Rust (build the nodejs WASM package first):
  [context/loro-js-rust-differential.md](../context/loro-js-rust-differential.md).
- Encoding formats: [context/internal-encoding.md](../context/internal-encoding.md)
  and `docs/encoding*.md`.

## Interop fixtures

- Rust output read by loro.js: `tests/fixtures/rust/*.blob` and `*.json`.
- loro.js output read by Rust: `pnpm fixtures:rewrite` runs
  `scripts/rewrite-rust-fixture.mjs`, which writes `tests/fixtures/rust/*.ts.blob`;
  `crates/loro/tests/loro_js_interop.rs` imports them. Container state encoders
  live in `src/runtime/document.ts` (`#containerState`) and must follow
  `docs/encoding-container-states.md`.

## Commands

From `loro-js/`: `pnpm test`, `pnpm typecheck`, `pnpm lint`, `pnpm format:check`,
`pnpm build`, or `pnpm check` for all of them. From the root, `pnpm test-loro-js`
runs the tests and fails if the Rust WASM reference is missing (CI runs it after
`release-wasm`). Benchmarks: `pnpm bench:b4`,
`pnpm bench:complexity -- 1000,2000,4000,8000`.
