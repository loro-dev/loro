# Repository Guidelines

## Project Snapshot

Loro is a Rust CRDT workspace with JS/WASM packaging and a MoonBit codec.

- `crates/loro`: public Rust API; avoid breaking downstream users.
- `crates/loro-internal`: core CRDT logic. Read its
  [AGENTS.md](crates/loro-internal/AGENTS.md) before changing import/export,
  encoding, state, diff, checkout, or replay behavior.
- `crates/loro-wasm`: `loro-crdt` WASM/TypeScript package. Read its
  [AGENTS.md](crates/loro-wasm/AGENTS.md) before changing bindings, exports,
  wrappers, or build scripts.
- `crates/delta`, `crates/rle`, `crates/kv-store`, `crates/fractional_index`,
  `crates/loro-common`, and `packages/fractional-index`: shared primitives and
  packages.
- `moon/`: MoonBit Loro binary codec; use [skills/moonbit/SKILL.md](skills/moonbit/SKILL.md).

## Context Index

- Encoding/import/export modes, current vs outdated formats, shallow snapshots:
  [context/internal-encoding.md](context/internal-encoding.md).
- Mergeable container model, marker/cid rules, tests, and common pitfalls:
  [context/mergeable-containers.md](context/mergeable-containers.md).
- User-facing Loro usage, sync, editor integration, and performance guidance:
  [skills/loro/SKILL.md](skills/loro/SKILL.md).
- Pure TypeScript runtime indexes, complexity contracts, benchmarks, and remaining gaps:
  [context/loro-js-performance.md](context/loro-js-performance.md).
- Rust/WASM, native Rust, and pure TypeScript differential fuzzing:
  [context/loro-js-interop-fuzz.md](context/loro-js-interop-fuzz.md).
- Context backlog: [context/CONTEXT-GAPS.md](context/CONTEXT-GAPS.md).

## Commands

- JS deps: `pnpm install --frozen-lockfile`.
- Rust build/check/format/lint: `cargo build`, `cargo check -p loro-internal`,
  `cargo fmt --all`, `pnpm check`.
- Rust tests: `pnpm test`; internal doctests: `cargo test -p loro-internal --doc`.
- Loom: `pnpm test-loom`.
- WASM: `pnpm release-wasm`, or `pnpm -C crates/loro-wasm build-dev`.
- Bundlers after WASM packaging changes: `pnpm test-bundlers`; browser runtime:
  `pnpm --dir examples/bundler-smoke-tests run test:browser`.
- Fractional-index TS: `pnpm test-fractional-index`.
- Fuzz smoke: `pnpm run-fuzz-corpus`.
- MoonBit codec, when `moon` is available: from `moon/`, run `moon check`,
  `moon test`, `moon fmt`.

Use narrow checks first. Ask before broad fuzzing or long browser matrices.

## Working Rules

- Start with `git status --short --branch`; treat uncommitted changes as user
  work unless you made them.
- Before editing, read every `AGENTS.md` from root to target directory. Keep
  `CLAUDE.md` as a symlink to the nearest `AGENTS.md`.
- Use `rg` / `rg --files` for search.
- Public API changes in `loro` or `loro-crdt` should be backward-compatible when
  possible. Prefer new `try_*` APIs over breaking signatures.
- Internal corruption should fail fast; invalid external input should return
  `Err`. Returning wrong state is worse than panicking on an impossible internal
  invariant.
- Add regression tests near behavior: `crates/loro/tests`,
  `crates/loro-internal/tests`, module tests, `crates/loro-wasm/tests`, or
  `moon/loro_codec/*_test.mbt`.
- Add a changeset for publishing behavior or package output changes.
- Do not hand-edit generated WASM package output; regenerate it with package
  scripts.

## Self-Maintained Agent Context

- Treat "why was that hard to find?" as a context bug. Add a nearby
  `AGENTS.md` pointer or a `context/` article, or append a line to
  [context/CONTEXT-GAPS.md](context/CONTEXT-GAPS.md).
- Keep root context short. If an `AGENTS.md` grows past about 4000 characters, move
  detail into a linked `context/` article.
- Header context articles with `Verified against code YYYY-MM-DD`, anchor claims
  to files/symbols, and link them from root plus the nearest per-directory
  `AGENTS.md`.
- If code changes make an `AGENTS.md` or context article stale, update the docs
  in the same change.
- When a commit needs non-obvious rationale, land that rationale in the nearest
  context file and keep the commit message as a pointer.

## Commit And PR Notes

History uses short imperative commits, often prefixed by `fix:`, `test:`,
`chore:`, or `refactor:`. PRs should include summary, rationale, validation, and
linked issues or traces when relevant.
