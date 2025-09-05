# Loro Binary Encoding Formats

Status: implementer-focused outline for Fast modes. Normative where code is clear; open items are marked for future formalization.

## Requirements

- Clearly describe Fast Snapshot, Fast Updates, and Shallow Snapshot.
- Be language-agnostic and step-by-step, suitable for reimplementation.
- Base content on the code in `crates/loro-internal` and `crates/kv-store` (no speculation).
- Include a primer on LEB128 varints used throughout.
- Call out limits and safety considerations for decoders.
- Track unresolved spec items as a markdown checklist.

## Introduction

Loro encodes documents and updates in compact binary formats designed for fast import/export and efficient sync. All formats share a simple file envelope, rely heavily on LEB128 varints, and reuse internal “arenas” (deduplication tables) and columnar strategies for compactness. This document covers the “Fast” modes:

- Fast Snapshot: complete or GC’d snapshot (oplog + state [+ shallow baseline]).
- Fast Updates: updates stream as peer-local, causally contiguous blocks.
- Shallow Snapshot: GC-friendly snapshot starting from a chosen frontier.

Reference implementation: see `crates/loro-internal/src/encoding.rs` and submodules.

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

- Integers: LEB128 (base-128) varints.
  - Unsigned LEB128 for lengths, counts, indices.
  - Signed LEB128 where deltas can be negative (e.g., timestamps Delta-of-Delta, integer deltas).
- Endianness:
  - Envelope mode: big-endian `u16`.
  - Section lengths in Fast Snapshot: little-endian `u32`.
  - `f64` payloads: 8-byte big-endian.
- Strings/Binary:
  - `[uLEB length][bytes]`. Strings are UTF‑8.
- Limits and safety:
  - Implementations SHOULD cap decoded sizes (e.g., `MAX_DECODED_SIZE`, `MAX_COLLECTION_SIZE`) to defend against malformed inputs.
  - Decoders MUST validate that reported lengths do not exceed remaining buffer size.

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

### EncodedBlock Schema (postcard-free)

This is the normative byte schema for a single block, designed for cross-language implementations without relying on `postcard`. The current Rust implementation packs the same inner payloads but wraps them in a `postcard` struct; encoders/decoders can adopt the schema below directly by concatenating the same inner bytes.

- `uLEB version`: block schema version. Must be `0`.
- `uLEB counter_start`: first counter in block.
- `uLEB counter_len`: number of counters covered by the block.
- `uLEB lamport_start`: first lamport in block.
- `uLEB lamport_len`: number of lamports covered by the block.
- `uLEB n_changes`: number of changes in the block.
- `uLEB header_len` + `header[header_len]`: compact “changes header” produced by `encode_changes`:
  - `uLEB peer_count` then `peer_count * 8-byte little-endian PeerID` (first is the block’s peer).
  - `n_changes-1` unsigned LEB128 atom lengths; the last length is `counter_len - sum(previous)`.
  - Dependency columns concatenated without extra length prefixes:
    - BoolRLE (exactly `n_changes` values): “dep on self” flags.
    - AnyRLE (exactly `n_changes` values): “other-deps count” per change.
    - AnyRLE (exactly sum of counts) of `peer_idx`, followed by DeltaOfDelta (same count) of `counter`.
  - Lamports: DeltaOfDelta (exactly `n_changes-1` values). The last lamport is derived as `lamport_start + lamport_len - last_change_len`.
- `uLEB change_meta_len` + `change_meta[change_meta_len]`: concatenation, no internal length headers:
  - DeltaOfDelta timestamps (`i64`, exactly `n_changes`).
  - AnyRLE commit message lengths (`u32`, exactly `n_changes`).
  - Commit message UTF‑8 bytes concatenated; individual sizes come from the previous stream.
- `uLEB cids_len` + `cids[cids_len]`: `serde_columnar` bytes for `ContainerArena` with strategies:
  - `is_root: BoolRle`, `kind: Rle(u8)`, `peer_idx: Rle(usize)`, `key_idx_or_counter: DeltaRle(i32)`.
- `uLEB keys_len` + `keys[keys_len]`: repeated `[uLEB utf8_len][utf8_bytes]` until buffer ends.
- `uLEB positions_len` + `positions[positions_len]`: `serde_columnar` `PositionArena` v2. Empty is allowed and decodes to empty.
- `uLEB ops_len` + `ops[ops_len]`: `serde_columnar` vector of `EncodedOp` with strategies:
  - `container_index: DeltaRle(u32)`, `prop: DeltaRle(i32)`, `value_type: Rle(u8)`, `len: Rle(u32)`.
- `uLEB delete_start_ids_len` + `delete_start_ids[delete_start_ids_len]`: `serde_columnar` vector of `EncodedDeleteStartId`; zero-length allowed.
- `uLEB values_len` + `values[values_len]`: contiguous value byte stream; ops consume in order (see Value Encoding).

Decoder outline:

- Read fields in order; for each length-prefixed payload, slice that many bytes.
- For `header` and `change_meta`, decode substreams using the exact counts above (no internal length prefixes).
- Use `serde_columnar` decoders for `cids`, `positions`, `ops`, `delete_start_ids`.
- Parse `keys` as repeated varint length + UTF‑8 bytes.
- Decode `values` using the per-op `value_type` with arenas.

Implementation notes:

- The stream framing provides `[uLEB block_len][block_bytes]` pairs; `block_bytes` is exactly the EncodedBlock v0 schema above.
- This schema makes fast range queries trivial: read the first five uLEB fields to get counter/lamport ranges, then skip or parse the rest.

Code references for the inner payloads and strategies: `oplog/change_store/block_encode.rs`, `oplog/change_store/block_meta_encode.rs`, and `encoding/arena.rs`.

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

## Arenas and Registers

Blocks deduplicate repeated items across columns via arenas.

- PeerIdArena: `[uLEB count][count * 8-byte peer_id BE]`.
- ContainerArena: serde_columnar vector of `EncodedContainer` entries, each with:
  - `is_root: BoolRle`, `kind: Rle u8`, `peer_idx: Rle usize`, `key_idx_or_counter: DeltaRle i32`.
  - Decoders reconstruct `ContainerID` using peer/key arenas.
- Key strings: concatenation of `[uLEB len][bytes]` (UTF‑8).
- TreeIDArena: serde_columnar `(peer_idx: Rle usize, counter: DeltaRle i32)`.
- PositionArena: serde_columnar of `(common_prefix_length: Rle usize, rest: bytes)` building prefix-compressed fractional positions. Empty slice encodes to empty bytes.
- DepsArena: serde_columnar vector of `(peer_idx: Rle usize, counter: DeltaRle i32)` iterated per change.

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
- Fast Updates import: read blocks → decode → sort by lamport → merge into DAG, handle pending deps.

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

- [x] Specify a postcard-free, normative byte schema for EncodedBlock (field order, varints, framing).
- [ ] Enumerate serde_columnar strategies in a table per field (RLE, DeltaRle, DeltaOfDelta) with examples.
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
