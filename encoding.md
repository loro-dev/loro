# Loro Binary Encoding Formats

Status: implementer-focused outline for Fast modes. Grounded in current code. Blocks encode fields with uLEB varints and length‑prefixed byte slices; columnar segments use serde_columnar.

## Requirements

- Clearly describe Fast Snapshot, Fast Updates, and Shallow Snapshot.
- Be language-agnostic and step-by-step, suitable for reimplementation.
- Base content on the code in `crates/loro-internal` and `crates/kv-store` (no speculation).
- Include a primer on LEB128 varints used throughout.
- Call out limits and safety considerations for decoders.
- Track unresolved spec items as a markdown checklist.
- Do not reference specific serialization libraries in normative text (no mentions of "postcard").
- Specify EncodedBlock in library-agnostic terms (uLEB integers + length‑prefixed byte slices) so it’s implementable without library knowledge.
- Where serde_columnar is used in code, list per-field strategies to aid reimplementation; do not require serde_columnar to understand the on-wire format.

## Introduction

Loro encodes documents and updates in compact binary formats designed for fast import/export and efficient sync. All formats share a simple file envelope, rely heavily on LEB128 varints, and reuse internal “arenas” (deduplication tables) and columnar strategies for compactness. This document covers the “Fast” modes:

- Fast Snapshot: complete or GC’d snapshot (oplog + state [+ shallow baseline]).
- Fast Updates: updates stream as peer-local, causally contiguous blocks.
- Shallow Snapshot: GC-friendly snapshot starting from a chosen frontier.

Reference implementation: see `crates/loro-internal/src/encoding.rs` and submodules. Treat the code as the source of truth.

## Conformance Notes

- MUST validate the envelope, including magic bytes, mode, and checksum.
- MUST treat LEB128 fields as little-endian varints (unsigned or signed as indicated).
- SHOULD enforce decoding limits (counts/lengths) to avoid resource exhaustion.
- SHOULD ignore/round-trip unknown “future” value kinds.

## Envelope

Every exported blob begins with the same fixed-size header.

- Magic: 4 bytes ASCII `loro`.
- Checksum: 16-byte area; semantics depend on mode:
  - Fast modes: last 4 bytes contain `xxhash32(body, seed = 0x4F524F4C /* "LORO" LE */)` in little-endian; the leading 12 bytes are zeroes.
  - Outdated modes: all 16 bytes contain `md5(body)`.
- Mode: 2-byte big-endian `u16`:
  - `1` = OutdatedRle (legacy updates)
  - `2` = OutdatedSnapshot (legacy snapshot)
  - `3` = FastSnapshot
  - `4` = FastUpdates
- Body: mode-specific payload immediately follows.

Parsing and checksum verification: `parse_header_and_body` in `encoding.rs`.

## Shared Conventions

- Varints: LEB128 (base-128) variable-length integers.
  - Unsigned (uLEB) for lengths, counts, and non-negative integers.
  - Signed (sLEB) where negative values may occur (deltas, i64 timestamps).
- Endianness:
  - Envelope mode tag: big-endian `u16`.
  - Fast Snapshot section lengths: little-endian `u32`.
  - `f64`: 8-byte big-endian IEEE-754.
- Primitive encodings:
  - `u8`: 1 byte.
  - `u32`, `usize`: uLEB.
  - `i32`, `isize`, `i64`: sLEB.
  - `bytes`: `[uLEB len][raw octets]`.
  - `str`: `[uLEB len][UTF-8 octets]` (no NUL terminator).
- Limits and safety:
  - Implementations SHOULD cap decoded sizes (collection lengths, column run counts, arena byte lengths) to defend against malformed inputs.
  - Decoders MUST validate that reported lengths do not exceed remaining buffer size.

### Column Codecs (serde_columnar)

Columns in blocks use serde_columnar encoders/decoders. Strategies used in code:

- Rle: run-length encoding for repeated scalars.
- DeltaRle: deltas (signed) over absolute values, then RLE.
- BoolRle: run-length encoding specialized for booleans.
- DeltaOfDelta: second-difference encoding for near-constant steps (timestamps/lamports/deps counters in headers).

Note: This document does not restate the byte-level algorithm of these strategies; interop requires a serde_columnar-compatible implementation.

### Column Packaging (vectors of rows)

A “Column‑Vec blob” encodes one vector of rows as a sequence header plus one element per column, then optional mapping pairs. This definition is self‑contained and does not assume any external library.

- Header: `uLEB elem_count` where `elem_count = F + 2*O`.
  - `F`: number of primary (non‑optional) fields.
  - `O`: number of optional fields that are present in this encoding.
- Primary columns (exactly `F` elements): for each field in declaration order, a BYTES element:
  - BYTES element: `[uLEB col_len][col_bytes]`, where `col_bytes` is produced by that field’s codec (RLE, DeltaRLE, etc.).
- Optional columns (exactly `O` pairs): for each present optional field, two elements appended:
  - `uLEB opt_field_index` (stable integer index for this optional field)
  - BYTES element: `[uLEB col_len][col_bytes]` for that optional column.

Notes:
- Row count is not transmitted separately; decoders derive it by decoding any primary column and must validate that all columns agree on length.
- In EncodedBlock usages, no optional columns are currently used; thus `elem_count = F`.

## Key–Value Bytes (KV Store)

Fast Snapshot and Shallow Snapshot carry KV-encoded stores for the oplog and state. The reference KV store is a simple SSTable-like format with prefix-compressed keys inside blocks and an xxhash32-guarded block trailer.

- Overview and rationale: `crates/kv-store/src/lib.rs`.
- Key/value chunk within a block uses prefix compression relative to the block’s first key:
  - First chunk: `[key_suffix bytes][value bytes]` (first key equals the block’s first key).
  - Subsequent chunks: `[u8 common_prefix_len][u16 key_suffix_len][key_suffix bytes][value bytes]`.
- Blocks are optionally LZ4-compressed and carry an xxhash32 checksum.
- A Block Meta section indexes blocks by first/last key and offset.

Note: Implementations may treat KV bytes as an opaque bytestream if a compatible KV implementation is provided. If reimplementing, follow the SSTable and chunk layout above.

## Fast Snapshot

Body is three length-prefixed sections. Each length is a little-endian `u32` and is immediately followed by that many bytes.

1) `[u32 oplog_len][oplog_bytes]`
2) `[u32 state_len][state_bytes_or_E]`
   - If `state_len == 1` and the single byte is ASCII `E` (0x45), state is absent and must be derived.
3) `[u32 shallow_len][shallow_root_state_bytes]`

Semantics:

- `oplog_bytes`: KV export of the `ChangeStore` (see Fast Updates and “Blocks”). Contains all encoded change blocks and metadata keys (`vv`, `fr`, optionally shallow start `sv`, `sf`).
- `state_bytes_or_E`:
  - Present: KV export of the full, latest state (all alive containers), enabling import without replay.
  - Single `E`: no latest state provided; latest must be computed by replay (starting from baseline when present).
- `shallow_root_state_bytes`:
  - Empty: full snapshot; apply `state_bytes` if present.
  - Non-empty: GC baseline (baseline frontiers are stored under key `fr` in this KV). Importers load baseline, then either merge `state_bytes` or replay updates since baseline.

Import outline:

- Parse sections; decode `oplog_bytes` into the `ChangeStore` (blocks + vv/frontiers).
- If `shallow_root_state_bytes` is empty: load `state_bytes` when present.
- If non-empty: decode baseline via `decode_gc`, then either:
  - Merge baseline + `state_bytes` (`decode_state_by_two_bytes`), or
  - Replay changes to compute latest if `state_bytes` is absent.

Code: `encoding/fast_snapshot.rs` and `state/container_store.rs`.

## Fast Updates

The body is a concatenation of length-prefixed blocks. Each block encodes a causally contiguous set of changes for a single peer.

- Stream framing: `... [uLEB block_len][block_bytes] [uLEB block_len][block_bytes] ...` until EOF.

### EncodedBlock (current wire format)

The Fast Updates body is a stream of `[uLEB block_len][block_bytes]` pairs. Each `block_bytes` encodes fields in this exact order using uLEB varints for integers and `[uLEB len][bytes]` for byte slices.

Fields and framing (in order):

- `counter_start: u32`, `counter_len: u32`: first counter and covered length.
- `lamport_start: u32`, `lamport_len: u32`: first lamport and covered length.
- `n_changes: u32`: number of changes.
- `header: [uLEB len][len bytes]`: produced by `encode_changes` (see below).
- `change_meta: [uLEB len][len bytes]`: produced by `encode_changes` (timestamps + commit messages).
- Ops payloads (all as length‑prefixed byte slices):
  - `cids`: serde_columnar bytes for `ContainerArena`.
  - `keys`: concatenation of `[uLEB len][utf8]` entries.
  - `positions`: serde_columnar bytes for `PositionArena::encode_v2()`; may be empty to mean zero rows.
  - `ops`: serde_columnar bytes for `EncodedOps` (ops columns).
  - `delete_start_ids`: serde_columnar bytes for `EncodedDeleteStartIds`; may be empty if none.
  - `values`: contiguous value stream consumed in op order (see Value Encoding).

Header bytes (`header`):

- `uLEB peer_count`, followed by `peer_count` PeerIDs as 8-byte little-endian integers (first is the block’s peer).
- `n_changes-1` change atom lengths (uLEB); the last length is `counter_len − sum(previous)`.
- Dependencies (concatenated substreams):
  - `BoolRle` of `dep_on_self` (n_changes entries).
  - `AnyRle<usize>` of `deps_len` per change (n_changes entries).
  - `AnyRle<usize>` of `peer_idx` for all “other” deps (sum of `deps_len`).
  - `DeltaOfDelta<u32>` of `counter` for those deps (same count).
- Lamports: `DeltaOfDelta<i64>` for the first `n_changes-1` lamports. The final lamport is derived as `lamport_start + lamport_len - last_change_len`.

Change meta bytes (`change_meta`):

- `DeltaOfDelta<i64>` timestamps (exactly `n_changes`).
- `AnyRle<u32>` commit message lengths (n_changes), followed by concatenated UTF‑8 bytes.

Serde-columnar segments and field strategies:

- `EncodedOps` rows:
  - `container_index: DeltaRle u32`
  - `prop: DeltaRle i32`
  - `value_type: Rle u8`
  - `len: Rle u32`
- `EncodedDeleteStartId` rows:
  - `peer_idx: DeltaRle usize`
  - `counter: DeltaRle i32`
  - `len: DeltaRle isize`
- `ContainerArena` rows:
  - `is_root: BoolRle`, `kind: Rle u8`, `peer_idx: Rle usize`, `key_idx_or_counter: DeltaRle i32`
- `PositionArena` rows:
  - `common_prefix_length: Rle usize`, `rest: bytes` (prefix-compressed fractional index suffix)

Notes:
- Quick range checks should use `block_len` to skip entire blocks efficiently.
- To parse a block, read the five uLEB integers first, then read each subsequent segment as `[uLEB len][len bytes]` in the order above.

Implementation notes:

- The stream framing provides `[uLEB block_len][block_bytes]` pairs.
- For fast skipping, use `block_len`. To inspect ranges or decode payloads, parse the uLEB integers and the length‑prefixed segments in order.

Code references for the inner payloads and layouts: `oplog/change_store/block_encode.rs`, `oplog/change_store/block_meta_encode.rs`, and `encoding/arena.rs`.

## Value Encoding

Operation payloads are encoded into a compact, typed stream. The per-op `value_type` is a single byte; content follows in the `values` segment and is decoded with help from arenas.

- Primitive kinds:
  - `Null` (tag only)
  - `True`/`False` (tag only)
  - `I64` (sLEB128)
  - `F64` (8-byte BE)
  - `Str` and `Binary`: `[uLEB len][bytes]`
- Containers:
  - `ContainerType` — encodes a container type discriminator; concrete IDs come from the `ContainerArena` by index.
- Composite `LoroValue`:
  - List: `[uLEB len]` then nested values.
  - Map: `[uLEB len]` then repeated `[uLEB key_index][value]`.
- CRDT-specific payloads (selected):
  - `MarkStart`: `[u8 info][uLEB len][uLEB key_index][value]`.
  - `ListMove`: `{from, from_idx, lamport} as uLEB`.
  - `ListSet`: `{peer_idx, lamport as uLEB, value}`.
  - `TreeMove`/`RawTreeMove`: indices into peer/position/tree arenas plus flags.
- Future-proofing:
  - Unknown kinds set the high bit (`0x80`) and carry raw `[uLEB len][bytes]`. Decoders MUST preserve/round-trip unknown kinds.

Code: `encoding/value.rs`.

## Column Layouts (serde_columnar)

This section lists the serde_columnar strategies used by each column in EncodedBlock. See code for exact serialization; interoperable implementations should match serde_columnar’s on-wire format.

- EncodedOp: `container_index: DeltaRle u32`, `prop: DeltaRle i32`, `value_type: Rle u8`, `len: Rle u32`.
- ContainerArena: `is_root: BoolRle`, `kind: Rle u8`, `peer_idx: Rle usize`, `key_idx_or_counter: DeltaRle i32`.
- EncodedDeleteStartId: `peer_idx: DeltaRle usize`, `counter: DeltaRle i32`, `len: DeltaRle isize`.
- PositionArena: `common_prefix_length: Rle usize`, `rest: bytes` (prefix-compressed suffix).

## Arenas and Registers

Blocks deduplicate repeated items across columns via arenas. For Fast Updates blocks:

- ContainerArena: serde_columnar-encoded `EncodedContainer` rows. Decoders reconstruct `ContainerID` using the peer list from the block header and the key strings from `keys`.
- Key strings (`keys` field): concatenation of `[uLEB len][bytes]` (UTF‑8).
- PositionArena: serde_columnar-encoded prefix-compressed positions; empty bytes means zero rows.

Other arenas (PeerIdArena, TreeIDArena, DepsArena) are used elsewhere in the codebase but are not serialized inside Fast Updates blocks.

Code: `encoding/arena.rs`.

## Shallow Snapshot

Shallow snapshots provide a GC baseline and only the recent history required to reach the latest state.

- Start frontier: computed as LCA of the requested frontier and the latest frontiers. For rich-text, `StyleStart`/`StyleEnd` pairs are kept together: if `start` lands on a `StyleStart`, advance by one to include its matching end.
- Exports three sections using the same Fast Snapshot framing:
  - `oplog_bytes`: change-store KV export from `start_vv` (inclusive) to latest; includes start/end metadata keys.
  - `shallow_root_state_bytes`: KV export of the baseline state at `start_frontiers` with an embedded `fr` key recording that frontier.
  - `state_bytes` (optional): if too many ops since baseline, export a delta state so import can avoid replay.
- Import mirrors Fast Snapshot with GC:
  - Load `shallow_root_state_bytes`, then either merge the delta `state_bytes` or replay updates from `oplog_bytes`.

Code: `encoding/shallow_snapshot.rs`.

## Decoding/Encoding Flows

- Fast Snapshot export: envelope → write three sections → compute xxhash32 → store in header (last 4 checksum bytes LE).
- Fast Snapshot import: parse envelope/checksum → decode `ChangeStore` → load baseline/state → if needed, replay to latest.
- Fast Updates export: diff vs input VV → build peer-contiguous blocks → emit `[uLEB len][block]`.
- Fast Updates import: read blocks → parse uLEB fields and segments → decode substreams → sort by lamport → merge into DAG.

## Checksums and Versioning

- Envelope checksum:
  - Fast modes: `xxhash32(body, seed = "LORO")` in last 4 checksum bytes LE; preceding 12 bytes are zero.
  - Outdated modes: full 16-byte MD5 of body.
- Block version: the block payload begins with an unsigned LEB128 version field; current value is `0`. Importers MUST reject unknown block versions.

## LEB128 Primer

- Unsigned: encode 7 bits per byte, set MSB=1 for continuation, MSB=0 for last byte.
  - Example: `300 -> [0b10101100, 0b00000010]`.
- Signed: same layout with sign extension; negative values propagate the sign bit when decoding.
- Usage in Loro:
  - Lengths/counts/indices: unsigned LEB128.
  - Deltas: signed LEB128 (e.g., timestamps, integer deltas).

## Security and Limits

- Enforce caps on all decoded sizes (collections, arena lengths, KV entries).
- Validate lengths before slicing; treat out-of-range indices as corruption.
- Unknown value kinds must be preserved (forward compatibility).
- Do not trust commit message lengths/UTF‑8 without validation; reject invalid UTF‑8.

## Open Items

- [ ] Add worked examples (hex dumps) of small blocks for implementers.
- [ ] Add worked examples (hex dumps) of small blocks for implementers.
- [ ] Document rich-text StyleStart/StyleEnd pairing in shallow frontier calculation with concrete examples.
- [ ] Finalize recommended decoding limits (MAX_* caps) with interoperable values.
- [ ] Compile an endianness matrix and associated tests.

## References

- Envelope, modes, and dispatch: `crates/loro-internal/src/encoding.rs`.
- Fast Snapshot/Updates and metadata: `crates/loro-internal/src/encoding/fast_snapshot.rs`.
- Shallow Snapshot and state-only: `crates/loro-internal/src/encoding/shallow_snapshot.rs`.
- Change store and block encoding: `crates/loro-internal/src/oplog/change_store.rs` and submodules.
- Arenas and values: `crates/loro-internal/src/encoding/arena.rs`, `crates/loro-internal/src/encoding/value.rs`.
- KV store format: `crates/kv-store/src/lib.rs`.
