# loro.js

Pure TypeScript implementation of Loro (`loro-js/src`). It must read and write
the same binary and JSON data as Rust (`loro-crdt`) and give the same results.

## Context

- Index structures, complexity contracts, and benchmarks:
  [context/loro-js-performance.md](../context/loro-js-performance.md).
- Randomized Text comparison with Rust (build the nodejs WASM package first):
  [context/loro-js-rust-differential.md](../context/loro-js-rust-differential.md).
- Multi-peer fuzzing of all containers against replay, checkout, event, and
  shallow-snapshot oracles, optionally in lockstep with Rust:
  [context/loro-js-differential-fuzz.md](../context/loro-js-differential-fuzz.md).
- Rich text: style anchors live in the Text sequence and op positions count
  them. Read [context/loro-js-richtext-anchors.md](../context/loro-js-richtext-anchors.md)
  before changing Text positions, marks, or Text snapshot state.
- Encoding formats: [context/internal-encoding.md](../context/internal-encoding.md)
  and `docs/encoding*.md`.

## Interop fixtures

- Rust output read by loro.js: `tests/fixtures/rust/*.blob` and `*.json`.
- loro.js output read by Rust: `pnpm fixtures:rewrite` runs
  `scripts/rewrite-rust-fixture.mjs`, which writes `tests/fixtures/rust/*.ts.blob`;
  `crates/loro/tests/loro_js_interop.rs` imports them. Container state encoders
  live in `src/runtime/document.ts` (`#containerState`) and must follow
  `docs/encoding-container-states.md`.
- loro.js 0.2 output read by later versions: `scripts/write-legacy-fixtures.mjs`
  writes `tests/fixtures/loro-js-0.2/` with a 0.2 build; `tests/legacy-data.test.ts`
  pins that loro.js and Rust read it the same way. Later versions read 0.2 data
  with Rust's semantics, with no version marker (README "Upgrading from 0.2"; the
  decision is in [context/loro-js-rust-differential.md](../context/loro-js-rust-differential.md)).

## Commands

From `loro-js/`: `pnpm test`, `pnpm typecheck`, `pnpm lint`, `pnpm format:check`,
`pnpm build`, or `pnpm check` for all of them. From the root, `pnpm test-loro-js`
runs the tests and fails if the Rust WASM reference is missing (CI runs it after
`release-wasm`). Benchmarks: `pnpm bench:b4`,
`pnpm bench:complexity -- 1000,2000,4000,8000`.
