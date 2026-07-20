# Rust / TypeScript interoperability fuzzing

Verified against code 2026-07-19.

The executable harness lives under `loro-js/fuzz/`. Its shared scenario schema
is defined in `scenario.mjs`, and `runner.mjs` is the semantic oracle for both
the Rust/WASM package and the pure TypeScript runtime.

For every command, `runDifferentialScenario` crosses transport blobs between
the two implementations and compares canonical state, history metadata, and
events. `runMalformedImportChecks` mutates Rust- and TypeScript-produced current
update/snapshot blobs and requires matching acceptance or rejection; rejected
imports must not partially change the target. Raw encoded bytes are not compared
because current Loro encodings are wire-compatible but non-canonical.

`crates/fuzz/src/bin/loro_js_interop_driver.rs` consumes the same JSON schema.
When `LORO_INTEROP_NATIVE_DRIVER` is set, the Node runner checks JS-produced
blobs in native Rust and native-Rust-produced blobs in JS for every permanent
corpus scenario.

The stable generator avoids semantic no-op and invalid operations whose oplog
policy differs independently of visible CRDT behavior. It also omits rich-text
mark/unmark operations and does not compare events when a snapshot is imported
into a non-empty document. The strict corpus records these known gaps. In
particular, `fuzz/corpus/strict/richtext-anchor.json` demonstrates the remaining
anchor/entity-position mismatch after a marked range receives a later insert.

Event normalization is payload-only. Subscription callbacks enqueue raw event
batches, which are normalized after the public command returns. Do not query the
live document from a callback: the WASM adapter flushes queued callbacks at the
outer JS method boundary while the pure TypeScript runtime emits during the
internal commit, so callback-time state can differ even when event payloads and
the resulting document state agree.

Map event recording is operation-aware rather than only comparing the values at
the start and end of a batch. If a key visibly changes and is then restored in
the same local or imported change, Rust reports the final value in `updated`, so
the TypeScript runtime must retain that key. An incoming same-value winner that
never changes the visible value remains an event no-op. The regression scenario
is `fuzz/corpus/local-map-delete-restore-event.json`.

Run the complete green smoke from the repository root with:

```sh
pnpm test-loro-js-interop
```

See `loro-js/fuzz/README.md` for profiles, replay controls, and corpus promotion.
