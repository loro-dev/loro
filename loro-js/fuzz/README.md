# Rust / TypeScript interoperability fuzzing

This suite executes one JSON scenario against the Rust/WASM `loro-crdt`
binding and the pure TypeScript `loro-js` runtime. After every command it
compares canonical document state, deep values with container IDs, text delta,
tree state, version/frontier data, pending/history counts, and normalized event
batches.

Transport commands deliberately cross the implementations: Rust imports the
TypeScript blob and TypeScript imports the Rust blob. Both update and snapshot
formats are covered, along with duplicate and out-of-order delivery, checkout,
attach, and fresh-document round trips. A separate native Rust driver replays
the checked-in corpus in both directions so the oracle does not depend only on
the WASM binding. Deterministically corrupted update/snapshot inputs also check
matching acceptance or rejection and failure atomicity.

## Run

Build the Rust/WASM package once, then run the normal local profile:

```sh
pnpm release-wasm
pnpm --filter loro-js fuzz:interop
```

The CI profile runs 500 traces of at most 60 commands. The native profile runs
the permanent corpus through `crates/fuzz/src/bin/loro_js_interop_driver.rs`.

```sh
pnpm --filter loro-js fuzz:interop:ci
pnpm --filter loro-js fuzz:interop:native
```

Useful environment variables are:

- `LORO_INTEROP_FUZZ_RUNS`
- `LORO_INTEROP_FUZZ_MAX_COMMANDS`
- `LORO_INTEROP_FUZZ_TIME_MS`
- `LORO_INTEROP_FUZZ_SEED`
- `LORO_INTEROP_FUZZ_PATH`

Fast-check writes the shrunk scenario to `fuzz/artifacts/` and reports its
seed/path. Replay any scenario directly:

```sh
node loro-js/fuzz/run-interop-fuzz.mjs --replay /absolute/path/to/failure.json
```

Promote useful minimized failures into `fuzz/corpus/`.

## Profiles and oracle boundaries

The default `stable` profile targets the interoperability surface currently
claimed by `loro-js`. It chooses valid UTF-16 boundaries and effective
map/tree/movable-list operations so a failure is about CRDT behavior rather
than whether an implementation records an invalid or semantic no-op call.
Encoded byte lengths and layouts are never compared because Loro encoding is
not canonical.

Equivalent event representations are normalized: adjacent delta fragments are
coalesced, insert/delete operations at one cursor are put in a stable order,
and transaction-local tree create/delete pairs are treated as a net no-op. The
normalizer only reads event payloads; it never queries live document state from
inside a subscription callback, because the WASM wrapper and pure TypeScript
runtime can reach the callback at different points within one public method.
Snapshot imports into a non-empty document still have known event differences,
so the stable profile compares their resulting state but not their event list.

The `strict` profile adds rich-text mark/unmark generation, the corpus under
`fuzz/corpus/strict/`, and exact snapshot-import event comparison:

```sh
pnpm --filter loro-js fuzz:interop:strict -- --corpus-only
```

It currently reproduces the documented rich-text-anchor limitation: after a
mark, Rust encodes later text insert positions in the anchor-bearing entity
coordinate space while the pure TypeScript runtime uses visible Unicode
positions. Keep the strict corpus failing until that implementation gap is
fixed; it is intentionally not part of the green CI gate.
