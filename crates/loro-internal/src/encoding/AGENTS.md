# Encoding Guidelines

This module owns Loro import/export formats. Read
[../../../../context/internal-encoding.md](../../../../context/internal-encoding.md)
for the verified map of supported modes, outdated modes, shallow snapshots, JSON
schema, and validation entry points.

## Local Entry Points

- `../encoding.rs`: `ExportMode`, `EncodeMode`, 22-byte `loro` header, checksum
  validation, top-level dispatch, and `decode_import_blob_meta`.
- `fast_snapshot.rs`: current `FastSnapshot` and `FastUpdates` body layouts.
- `shallow_snapshot.rs`: `ShallowSnapshot`, `StateOnly`, and `SnapshotAt`.
- `json_schema.rs`: JSON updates, peer compression, validation, import/export,
  and redaction.
- `outdated_encode_reordered.rs`: legacy-named op/value columnar helpers still
  used by current fast paths; do not confuse this with unsupported top-level
  outdated blob modes.
- `value.rs`, `value_register.rs`, `arena.rs`: op/value encoding support.

## Rules

- Current binary modes are `FastSnapshot = 3` and `FastUpdates = 4`.
- Top-level `OutdatedRle = 1` and `OutdatedSnapshot = 2` are compatibility
  detections, not formats to extend.
- Malformed bytes or JSON schema should return `Err`, not partially import.
- Snapshot import/export must preserve rollback and attached/detached state
  invariants.
- Unknown container types must block shallow/state snapshot export rather than
  producing a blob that cannot be decoded correctly.

## Validation

- `cargo test -p loro-internal import_atomicity`
- `cargo test -p loro-internal decode_updates_rejects_truncated_block`
- `cargo test -p loro-internal --test mergeable_container` when snapshot changes
  can affect mergeable child retention.
- Root `pnpm test` for shared import/export semantic changes.
