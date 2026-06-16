# loro-internal Guidelines

This crate contains Loro's unstable internal CRDT implementation. Public API
compatibility concerns still matter because `crates/loro` and `crates/loro-wasm`
wrap this crate directly, but the internal priority is preserving invariants
over graceful degradation.

## Internal Map

- `src/loro.rs`: document-level orchestration for commit, import/export,
  checkout, barriers, state/oplog coordination, and event emission.
- `src/encoding.rs`: public/internal `ExportMode`, binary header parsing,
  checksum verification, `EncodeMode` dispatch, import metadata, and the bridge
  from decoded changes into `OpLog`.
- `src/encoding/`: concrete binary and JSON encoding implementations. Read
  `src/encoding/AGENTS.md` and
  [../../context/internal-encoding.md](../../context/internal-encoding.md)
  before changing binary layout, JSON schema, import metadata, shallow snapshot,
  or op/value encoding.
- `src/oplog/` and `src/dag/`: change storage, dependency ordering, pending
  changes, version vectors/frontiers, shallow roots, and history traversal.
- `src/state.rs` and `src/state/`: materialized document state, container stores,
  diff application, checkout/replay, deep value, dead-container tracking, and
  mergeable container visibility. Read `src/state/AGENTS.md` and
  [../../context/mergeable-containers.md](../../context/mergeable-containers.md)
  before changing mergeable containers.
- `src/handler.rs`: typed container handlers, local operation creation, and
  `MapHandler::ensure_mergeable_*`.
- `src/diff_calc/`: diff calculation when moving between versions.
- `docs/diff_calc.md`: design notes for diff calculation.
- `docs/mergeable-container-id.md`: current mergeable container id encoding.
- `tests/mergeable_container/` and `tests/mergeable_cid_encoding.rs`: focused
  mergeable container regression tests.
- `src/tests/import_atomicity.rs`: import rollback and malformed-input
  regressions.

## Commands

Use narrow checks first:

- `cargo check -p loro-internal`
- `cargo test -p loro-internal --doc`
- `cargo test -p loro-internal --test mergeable_container`
- `cargo test -p loro-internal --test mergeable_cid_encoding`
- `cargo test -p loro-internal import_atomicity`

For broad shared behavior, run the root commands from `AGENTS.md`. For changes
to import, checkout, encoding, state replay, or diff calculation, consider fuzz
coverage under `crates/fuzz` and ask before running long fuzz targets.

## Working Rules

- Internal invariant violation should fail fast. Invalid external bytes or JSON
  should return `Err`.
- Do not silently skip ops, containers, state entries, diffs, or pending changes.
- Snapshot/import paths must be atomic: if decode or state application fails,
  rollback must leave the document usable.
- Preserve attached/detached document state when export paths temporarily
  checkout another version.
- If a change affects `crates/loro` or `crates/loro-wasm` behavior, add or update
  tests at the wrapper layer as well as the internal layer when practical.
