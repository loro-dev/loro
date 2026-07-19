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

Run the complete green smoke from the repository root with:

```sh
pnpm test-loro-js-interop
```

See `loro-js/fuzz/README.md` for profiles, replay controls, and corpus promotion.
