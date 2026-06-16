# Internal Encoding Context

Verified against code 2026-06-16.

Loro has one binary blob envelope, two current binary body formats, two
recognized-but-unsupported legacy top-level modes, and a separate JSON updates
schema. The most common mistake is to treat `outdated_encode_reordered.rs` as an
obsolete file; only top-level blob modes 1 and 2 are obsolete. Several helpers in
that file are still used by current fast paths.

## Two-Hop Answer

If an agent asks "how does Loro encoding work?", start here:

- [crates/loro-internal/src/encoding.rs](../crates/loro-internal/src/encoding.rs):
  `ExportMode`, `EncodeMode`, `parse_header_and_body`, `encode_with`,
  `decode_oplog_changes`, `decode_snapshot`, `decode_import_blob_meta`.
- [crates/loro-internal/src/loro.rs](../crates/loro-internal/src/loro.rs):
  `LoroDoc::_import_with` chooses snapshot-vs-updates application behavior.
- [crates/loro-internal/src/encoding/fast_snapshot.rs](../crates/loro-internal/src/encoding/fast_snapshot.rs):
  `Snapshot`, `encode_snapshot_inner`, `decode_snapshot_inner`, `encode_updates`,
  `decode_updates`.
- [crates/loro-internal/src/encoding/shallow_snapshot.rs](../crates/loro-internal/src/encoding/shallow_snapshot.rs):
  `export_shallow_snapshot_inner`, `export_state_only_snapshot`,
  `encode_snapshot_at`.
- [crates/loro-internal/src/encoding/json_schema.rs](../crates/loro-internal/src/encoding/json_schema.rs):
  `JsonSchema`, `export_json`, `decode_changes`, `redact`.
- [docs/encoding.md](../docs/encoding.md) and
  [docs/encoding-container-states.md](../docs/encoding-container-states.md):
  external binary format references. Verify against code before changing them.

## Binary Envelope

Every binary export starts with:

- magic bytes `loro` from `encoding.rs:MAGIC_BYTES`,
- a 16-byte checksum field,
- a big-endian `u16` `EncodeMode`,
- mode-specific body bytes.

For current `FastSnapshot` and `FastUpdates` blobs, `ParsedHeaderAndBody::check_checksum`
uses `xxhash32` over bytes starting at offset 20, which includes the mode bytes
and body. Legacy modes use the older MD5 check path only for detection.

## Supported And Outdated Modes

Current modes:

- `EncodeMode::FastSnapshot = 3`: used by `ExportMode::Snapshot`,
  `ShallowSnapshot`, `StateOnly`, and `SnapshotAt`.
- `EncodeMode::FastUpdates = 4`: used by `ExportMode::Updates` and
  `UpdatesInRange`.

Recognized but unsupported top-level modes:

- `EncodeMode::OutdatedRle = 1`
- `EncodeMode::OutdatedSnapshot = 2`

`encoding.rs:decode_oplog_changes`, `encoding.rs:decode_snapshot`, and
`LoroDoc::decode_import_blob_meta` return `ImportUnsupportedEncodingMode` for
these outdated top-level modes. Do not extend them without compatibility
fixtures and a migration plan.

Important nuance: [outdated_encode_reordered.rs](../crates/loro-internal/src/encoding/outdated_encode_reordered.rs)
still contains current helpers including `import_changes_to_oplog`, `encode_op`,
`decode_op`, and `ValueRegister`.

## FastSnapshot

`fast_snapshot.rs:Snapshot` has three body sections:

1. `oplog_bytes`: KV-store encoded change history.
2. `state_bytes`: KV-store encoded materialized state, or `EMPTY_MARK` when
   omitted and state must be recalculated.
3. `shallow_root_state_bytes`: KV-store encoded shallow root state; empty for a
   non-shallow snapshot.

`decode_snapshot_inner` only initializes directly when importing into an empty
document. If a snapshot is imported into a non-empty document,
`LoroDoc::_import_with` routes through decoded oplog changes instead. Failed
direct snapshot import must reset both state and oplog.

## FastUpdates

`FastUpdates` is a sequence of LEB128 length-prefixed change blocks.
`fast_snapshot.rs:decode_updates` rejects invalid block lengths, length
overflow, and truncated block payloads, then sorts decoded changes by lamport.
`encoding.rs:apply_decoded_changes_to_oplog` imports changes, separates pending
changes, applies newly-unlocked pending changes, and rejects dependencies before
a shallow root.

## Shallow, State-Only, And SnapshotAt

All three use `FastSnapshot` mode:

- `ShallowSnapshot` retains history since a calculated shallow start frontier.
- `StateOnly` is a shallow snapshot with minimal history at the target version.
- `SnapshotAt` exports full history up to target frontiers plus state at that
  version.

`shallow_snapshot.rs` temporarily checks out versions and must restore the
document's original state and attached/detached status. It must not split rich
text style start/end ops across the shallow root. Unknown container types block
shallow/state snapshot export through `LoroEncodeError::UnknownContainer`.

## JSON Updates

`json_schema.rs` is not wrapped in the binary `loro` envelope. Its
`JsonSchema` carries:

- `schema_version = 1`,
- `start_version`,
- optional peer compression table,
- JSON changes and ops.

Malformed JSON schema should return `Err` without partial import. Look at
[crates/loro-internal/src/tests/import_atomicity.rs](../crates/loro-internal/src/tests/import_atomicity.rs)
when changing JSON import validation or rollback behavior.

## Validation Shortcuts

- Binary malformed input or rollback: `cargo test -p loro-internal import_atomicity`
- Truncated fast updates: `cargo test -p loro-internal decode_updates_rejects_truncated_block`
- Snapshot retention that might involve mergeable containers:
  `cargo test -p loro-internal --test mergeable_container`
- Shared behavior: root `pnpm test`

## Common Misconceptions

- "Outdated modes are still supported because `LoroDoc::_import_with` branches on
  them." They are detected, then route to decode paths that return unsupported.
- "`outdated_encode_reordered.rs` is dead." It is legacy-named but still contains
  active op/value helpers.
- "Snapshot import always initializes state directly." Only empty docs can reset
  from snapshot; non-empty imports use oplog-change application.
