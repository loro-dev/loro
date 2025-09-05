# Loro Encoding Architecture — Overview

This note summarizes how encoding is structured in `crates/loro-internal`, based on the touchpoints exposed from `src/lib.rs` and the `encoding` module it re-exports.

## Surface in `lib.rs`

- `pub mod encoding`: Exposes the full encoding module (`src/encoding.rs` and its submodules in `src/encoding/`).
- `pub use encoding::json_schema::json`: Re-exports the JSON schema types under `encoding::json_schema::json` for consumers.

`lib.rs` itself doesn’t implement encoding logic; it wires the module and re-exports JSON schema helpers.

## Module Map

- `src/encoding.rs`: Central façade for encoding/decoding. Defines modes, headers, and dispatch.
- `src/encoding/fast_snapshot.rs`: Fast binary snapshot/updates encoding (length-prefixed sections).
- `src/encoding/shallow_snapshot.rs`: Shallow (GC) snapshot helpers and state-only snapshot at a frontiers.
- `src/encoding/outdated_encode_reordered.rs`: Older columnar encoding for snapshots/updates (still supported for import/export and metadata).
- `src/encoding/arena.rs`: Shared arenas/registers used by the columnar encoder (peers, containers, keys, deps, positions, tree IDs, state blob arena).
- `src/encoding/value.rs`: Low-level typed value stream for op payloads (readers/writers, kind tags, forward-compatible “future” values).
- `src/encoding/value_register.rs`: Generic value-to-index registry used during arena construction.
- `src/encoding/json_schema.rs`: JSON export/import of changes, with optional peer-id compression.

## Blob Header and Modes

- Magic: `"loro"` (4 bytes), followed by a 16-byte checksum area, a 2-byte big-endian `EncodeMode`, then the body.
- `EncodeMode` values:
  - `OutdatedRle` (1): old columnar updates.
  - `OutdatedSnapshot` (2): old columnar snapshot.
  - `FastSnapshot` (3): new fast snapshot (oplog + state + shallow-state sections).
  - `FastUpdates` (4): new fast updates stream.
  - `Auto` is config-only (not serialized).
- Checksum:
  - Outdated modes: full MD5 of body occupies all 16 bytes.
  - Fast modes: 32-bit xxhash in the last 4 bytes of the 16-byte checksum area.
- Parsing: `parse_header_and_body` validates magic, extracts mode/body, and (optionally) verifies checksums.

## Export/Import Flow (Façade)

- Export entrypoints (selected examples):
  - `export_fast_snapshot(doc)` → `fast_snapshot::encode_snapshot`.
  - `export_fast_updates(doc, vv)` → `fast_snapshot::encode_updates`.
  - `export_shallow_snapshot(doc, frontiers)` → `shallow_snapshot::export_shallow_snapshot`.
  - Legacy: `export_snapshot(doc)` → columnar (`outdated_encode_reordered::encode_snapshot`).
- Import/Decode:
  - Oplog-only: `decode_oplog(oplog, parsed)` dispatches to mode-specific updates decoder, then resolves pending deps and lamports.
  - Snapshot: `decode_snapshot(doc, mode, body, origin)` dispatches to `outdated_encode_reordered::decode_snapshot` or `fast_snapshot::decode_snapshot` and initializes state.
- Metadata: `LoroDoc::decode_import_blob_meta(blob, check)` parses headers and returns `ImportBlobMetadata` (mode, partial VV range, timestamps, and change count). Fast/old modes have dedicated metadata decoders.

## Fast Snapshot/Updates (`fast_snapshot.rs`)

- Layout (little-endian u32 length + payload):
  1) oplog bytes
  2) state bytes (or a single-byte EMPTY mark)
  3) shallow-root state bytes (empty for full snapshot)
- Oplog bytes use the optimized `oplog::ChangeStore` block encoding.
- State bytes are the encoded KV store of the full state (or omitted when a shallow snapshot includes shallow-root state separately).
- Shallow snapshots are assembled via `shallow_snapshot` and decoded by merging oplog, shallow-root state, and optional full state.

## Shallow Snapshot and State-Only (`shallow_snapshot.rs`)

- Computes an optimal “start” as the LCA of the requested frontiers and the latest frontiers to ensure replayability.
- Produces:
  - `oplog_bytes`: changes since the start version.
  - `shallow_root_state_bytes`: KV dump of only “alive” container roots at start.
  - Optional `state_bytes`: full state delta when history from start is large.
- Handles edge cases (e.g., ensuring text `StyleStart`/`StyleEnd` pairs aren’t split across the boundary).
- Also supports state-only snapshot at an exact frontiers (minimal history + state at that version).

## Outdated Columnar Encoding (`outdated_encode_reordered.rs`)

- Columnar layout encoded via `serde_columnar` with arenas/registers to deduplicate:
  - `PeerIdArena`, `ContainerArena`, `KeyArena`, `DepsArena`, `TreeIDArena`, and `PositionArena`.
  - `PositionArena` delta-compresses fractional indices by common-prefix.
- Values are encoded into a contiguous “raw values” stream using `ValueWriter`/`ValueReader` with per-value `ValueKind` tags.
- Ops/changes are encoded with sorted ordering (by container, prop, peer, lamport) to improve locality.
- Decoding reconstructs ops per-peer (reverse counter order), then assembles `Change`s and resolves deps. Import yields `ImportChangesResult` which the oplog applies (including pending handling and lamport calculation).

## Arenas and Registers (`arena.rs`, `value_register.rs`)

- `ValueRegister<T>` maps arbitrary values to stable integer indices used across arenas.
- `EncodedRegisters` implements `ValueEncodeRegister` and holds registers for peers, keys, containers, tree IDs, and positions. Positions are first collected into a `HashSet`, then sorted and finalized to a `ValueRegister` to stabilize indices.
- `encode_arena`/`decode_arena` produce/consume a concatenated bytes blob of all arena sections (each prefixed by LEB128 length).
- `DecodedArenas` implements `ValueDecodedArenasTrait` to supply keys/peers and help materialize higher-level ops (e.g., tree ops) from compact forms.

## Value Stream (`value.rs`)

- Encodes individual op payloads with a tagged union:
  - Primitive kinds (`Null`, `True/False`, `I64`, `F64`, `Str`, `Binary`).
  - Structured kinds (`LoroValue`, `ContainerType` via `ContainerIdx`, `MarkStart`, list moves/sets, tree moves, raw tree moves).
  - `Future(..)`: forward-compatible bucket; stores opaque bytes and a kind tag for unknown/future extensions.
- `ValueWriter`/`ValueReader` handle compact encoding/decoding, including `LoroValue` collections (lists/maps) with safety caps (`MAX_COLLECTION_SIZE`).
- Tree ops are encoded compactly (`EncodedTreeMove`, `RawTreeMove`) and resolved with arenas (peer IDs, tree IDs, fractional indices).

## JSON Schema (`json_schema.rs`)

- Human-readable changes export/import with optional peer compression:
  - When enabled, peer IDs are replaced by small indices and a `peers` array is appended.
  - Converts all container IDs, IDs, IDLPs, and tree IDs accordingly.
- Provides `json` module re-exported by `lib.rs` so consumers can serialize/deserialize `JsonSchema`, `JsonChange`, and op enums.

## State Snapshot Encoder/Decoder Hooks

- `StateSnapshotEncoder`: per-container encoder that records the op-order for state reconstruction, checks ID span validity, and delegates to “op encoders” used by the selected mode.
- `StateSnapshotDecodeContext`: provides the stream of `OpWithId` and peer registry to container states to reconstruct from snapshot ops.

## Typical Call Paths

- Fast snapshot export: `encoding::export_fast_snapshot` → header + `fast_snapshot::encode_snapshot` (oplog + state + shallow-state) → writes body → xxhash checksum in header.
- Fast updates export: `encoding::export_fast_updates` → header + `fast_snapshot::encode_updates` (ChangeStore blocks).
- Import updates: `parse_header_and_body` → mode dispatch → decode changes → `import_changes_to_oplog` → resolve pending/lamports → `ImportStatus`.
- Import snapshot: header parse → mode dispatch → decode oplog/state/shallow-state → initialize `DocState`.

## Compatibility and Safety

- Forward compatibility for op payloads via `FutureValue` to avoid hard failures on unknown kinds.
- Limits on decoded sizes (`MAX_DECODED_SIZE`, `MAX_COLLECTION_SIZE`) to mitigate malformed inputs.
- Mode-specific checksums to catch corruption during transport/persistence.

This layout gives a fast path for day-to-day sync (`FastUpdates`), a compact, append-only binary snapshot for persistence (`FastSnapshot`), and a GC-friendly shallow form when only a recent prefix of history is retained. Older columnar formats remain supported for import/export and introspection.

