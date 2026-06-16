# Encoding Guidelines

This module owns Loro's import/export formats. It is easy to confuse the
top-level blob modes, the current fast binary layouts, the legacy helper module,
and the JSON schema path; keep those boundaries explicit.

## Entry Points

- `../encoding.rs`: `ExportMode`, `EncodeMode`, 22-byte `loro` header,
  checksum validation, top-level encode/decode dispatch, and
  `decode_import_blob_meta`.
- `fast_snapshot.rs`: current `FastSnapshot` and `FastUpdates` body encoding.
- `shallow_snapshot.rs`: `ShallowSnapshot`, `StateOnly`, and `SnapshotAt`
  variants built on `FastSnapshot`.
- `json_schema.rs`: JSON updates (`JsonSchema`, `schema_version = 1`), peer
  compression, JSON validation, import/export, and redaction.
- `outdated_encode_reordered.rs`: legacy-named op/value columnar helpers and
  `import_changes_to_oplog`. The top-level outdated blob modes are unsupported,
  but this file still contains helpers used by current fast paths.
- `value.rs`, `value_register.rs`, and `arena.rs`: value/op encoding support,
  value tables, peer/key registers, and arena-backed value decoding.
- `../../Encoding.md`: older high-level encoding notes. Treat it as background,
  not the source of truth when code disagrees.
- `../../../../docs/encoding.md`: detailed current binary format reference.
- `../../../../docs/encoding-container-states.md`: container state snapshot
  layouts used inside `FastSnapshot.state_bytes`.

## Supported Formats

Top-level binary blobs all start with:

- magic bytes `loro`,
- 16 checksum bytes,
- a big-endian `u16` encode mode,
- then mode-specific body bytes.

Current supported binary modes:

- `EncodeMode::FastSnapshot = 3`: used by `ExportMode::Snapshot`,
  `ShallowSnapshot`, `StateOnly`, and `SnapshotAt`.
- `EncodeMode::FastUpdates = 4`: used by `ExportMode::Updates` and
  `UpdatesInRange`.

Legacy top-level modes:

- `EncodeMode::OutdatedRle = 1`
- `EncodeMode::OutdatedSnapshot = 2`

These parse as known modes for compatibility detection but currently return
`ImportUnsupportedEncodingMode` on import/metadata decode. Do not re-enable them
without a compatibility plan and fixtures.

JSON update format:

- `json_schema.rs` is not wrapped in the binary `loro` header.
- It carries `schema_version = 1`, `start_version`, optional peer compression,
  and a list of JSON changes/ops.
- Malformed JSON schema should return `Err`, not partially import.

## FastSnapshot

`fast_snapshot.rs` encodes a snapshot body as three length-prefixed sections:

1. `oplog_bytes`: KV-store encoded change history.
2. `state_bytes`: KV-store encoded materialized state, or `EMPTY_MARK` when
   omitted and state must be recalculated.
3. `shallow_root_state_bytes`: KV-store encoded shallow root state, empty for
   non-shallow snapshots.

Importing a snapshot into an empty doc can initialize oplog and state directly.
Importing snapshot data into a non-empty doc goes through decoded oplog changes
instead. Failed snapshot import must roll back both oplog and state.

## FastUpdates

`FastUpdates` body is a sequence of LEB128 length-prefixed change blocks.
`decode_updates` must reject truncated blocks, length overflow, and corrupt block
payloads. Decoded changes are sorted by lamport before being applied.

## Shallow/State-Only Snapshots

`shallow_snapshot.rs` temporarily checks out versions to build shallow root
state and state deltas, then restores the original document state.

- `ShallowSnapshot` retains history since a calculated shallow start frontier.
- `StateOnly` is a shallow snapshot with minimal history at a target version.
- `SnapshotAt` exports full history up to target frontiers plus state there.
- Unknown container types must block shallow/state snapshot export rather than
  writing a blob that cannot be decoded correctly.
- Style start/end ops must not be split across shallow roots.

## Validation

For encoding changes, prefer focused fixtures and malformed-input tests. Useful
starting points:

- `cargo test -p loro-internal import_atomicity`
- `cargo test -p loro-internal decode_updates_rejects_truncated_block`
- `cargo test -p loro-internal --test mergeable_container` when snapshots may
  affect mergeable child retention.
- Root `pnpm test` when changing shared import/export semantics.
