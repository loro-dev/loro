# Moon Codec Fuzzing (Rust ↔ MoonBit)

This repo contains Rust-driven fuzzers that exercise the MoonBit codec implementation by round-tripping
real Loro data through the Moon CLI (compiled to JS) and validating the result in Rust.

These fuzzers are **not** `cargo-fuzz` targets. They are deterministic, seed-based test drivers that
produce reproducible artifacts on failure.

## Prerequisites

- Rust toolchain (use the repo’s `rust-toolchain`)
- Node.js (`node`)
- MoonBit (`moon`)

Environment variables used by the drivers/tests:

- `MOON_BIN`: path to the `moon` executable (default: `moon`)
- `NODE_BIN`: path to `node` (default: `node`)

Example (common local setup):

```sh
export MOON_BIN="$HOME/.moon/bin/moon"
export NODE_BIN="node"
```

## How the fuzzers work

All fuzzers follow the same pattern:

1. Generate a random-but-deterministic sequence of ops in Rust (`seed` controls randomness).
2. Produce a binary blob (Snapshot or Updates) and/or JSON schema from Rust.
3. Invoke MoonBit CLI (`moon/cmd/loro_codec_cli`) compiled to JS and run with Node.
4. Validate the result back in Rust.
5. On mismatch, write a repro case to `<out-dir>/case-<seed>/`.

The Moon CLI is built automatically by each driver via:

```sh
moon build --target js --release cmd/loro_codec_cli
```

## Fuzz drivers

### 1) Snapshot decode fuzz: `moon_snapshot_fuzz`

Purpose: Validate Moon’s snapshot decoding by comparing **deep JSON**.

What it tests:

- Rust generates a FastSnapshot (mode=3) blob.
- Moon decodes the snapshot and prints deep JSON (`export-deep-json`).
- Rust compares it with `doc.get_deep_value().to_json_value()`.

Run:

```sh
MOON_BIN="$HOME/.moon/bin/moon" NODE_BIN=node \
  cargo run -p loro --example moon_snapshot_fuzz -- \
    --seed 1 --iters 200 --ops 400 --commit-every 20 --peers 10
```

Repro on failure:

- The driver writes `snapshot.blob`, `expected.json`, and Moon outputs into:
  `moon_snapshot_fuzz_artifacts/case-<seed>/`
- Re-run the exact failing seed with `--iters 1`:

```sh
MOON_BIN="$HOME/.moon/bin/moon" NODE_BIN=node \
  cargo run -p loro --example moon_snapshot_fuzz -- \
    --seed <seed> --iters 1 --ops <ops> --commit-every <n> --peers <n>
```

### 2) JsonSchema → Updates encode fuzz: `moon_jsonschema_fuzz`

Purpose: Validate Moon’s `encode-jsonschema` (JsonSchema JSON → binary FastUpdates mode=4).

Why the oracle is “Rust updates” (not “original local doc”):

- Counter state uses `f64` accumulation; floating-point sums are **not associative**.
- Different (but valid) deterministic application orders can produce tiny `f64` differences.
- To avoid false negatives, the fuzzer compares Moon’s encoded updates against **Rust’s encoded updates**
  for the same `(start_vv -> end_vv)` range, by importing both and comparing the resulting state.

What it tests:

- Rust generates a document and chooses a deterministic `start_frontiers` (sometimes non-empty).
- Rust exports:
  - `schema.json` via `export_json_updates(start_vv, end_vv)`
  - `updates_rust.blob` via `ExportMode::Updates { from: start_vv }`
  - optionally `base_snapshot.blob` via `ExportMode::SnapshotAt { version: start_frontiers }`
- Moon encodes `schema.json` into `updates_moon.blob`.
- Rust imports `base_snapshot + updates_rust` and `base_snapshot + updates_moon` and compares:
  - deep JSON state
  - `oplog_vv()` (operation coverage)
  - selected richtext deltas (to catch mark/unmark regressions)

Run:

```sh
MOON_BIN="$HOME/.moon/bin/moon" NODE_BIN=node \
  cargo run -p loro --example moon_jsonschema_fuzz -- \
    --seed 1 --iters 300 --ops 400 --commit-every 20 --peers 10
```

Repro on failure:

- Look at `moon_jsonschema_fuzz_artifacts/case-<seed>/`.
- Re-run with the failing seed and `--iters 1`.

## “Higher confidence” running modes

For longer runs (recommended before merging codec changes):

- More peers + more ops:

```sh
MOON_BIN="$HOME/.moon/bin/moon" NODE_BIN=node \
  cargo run -p loro --example moon_jsonschema_fuzz -- \
    --seed 1000 --iters 200 --ops 1000 --commit-every 50 --peers 20
```

- Use `--release` for speed:

```sh
MOON_BIN="$HOME/.moon/bin/moon" NODE_BIN=node \
  cargo run -p loro --release --example moon_jsonschema_fuzz -- \
    --seed 1 --iters 2000 --ops 1000 --commit-every 50 --peers 20
```

## Adding coverage (how to extend)

When adding new fuzz ops, prefer:

- Ops that mutate different container types (Map/List/Text/Tree/MovableList/Counter).
- Cross-peer edits (switch peer IDs between commits).
- Non-empty `start_frontiers` ranges (incremental import correctness).
- UTF-8/UTF-16 boundary behavior for Text.
- Large tables for varint boundaries (keys/peers >= 128).

When a failure occurs:

- Always keep the failing artifact directory.
- Turn it into a deterministic regression test if possible (a small, minimal seed/case).

## Robustness (negative testing)

In addition to semantic fuzzing, there are e2e tests that ensure the Moon CLI:

- Rejects wrong document modes (e.g., decoding updates as snapshot).
- Rejects malformed/truncated inputs.
- Rejects invalid JsonSchema JSON for `encode-jsonschema`.

These tests are meant to catch panics/crashes and “accepting garbage input”.

