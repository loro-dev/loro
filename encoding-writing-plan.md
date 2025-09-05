Requirements

- Introduce the encoding formats of Loro, specifically Fast Snapshot, Fast Updates, and Shallow Snapshot.
- Be language-agnostic so readers can implement the formats in other languages.
- Ensure correctness by reading the source code; include references and avoid speculation.
- Keep it easy to read, breaking concepts down step by step; include an introduction to LEB128.
- Allow TODO items for areas needing future research or formalization.
- Provide an outline that can be extended with field-by-field details later.

Encoding Formats Overview (Outline)

Goal: Provide a language-agnostic, implementer-friendly outline of Loro’s encoding formats focusing on Fast Snapshot, Fast Updates, and Shallow Snapshot. This outline captures the structure, shared primitives, and decoding flow so other languages can interoperate. TODO markers highlight places where deeper, format-level details could be specified further.

**Scope**
- Fast Snapshot: current document (oplog + state + optional GC baseline).
- Fast Updates: update stream encoded in causal “blocks”.
- Shallow Snapshot: compact snapshot with GC baseline and minimal history/state.
- Legacy formats (OutdatedRle/OutdatedSnapshot) are excluded from this outline.

**Terminology**
- Oplog: The change history (operations grouped as changes) with a causal DAG.
- Snapshot: Encapsulates history (oplog) and state (container store) in one blob.
- Shallow Root/Baseline: A GC baseline state at an earlier frontier to trim old history.
- Block: A causally and contiguously grouped set of changes for a single peer.
- LEB128: Variable-length integer encoding (unsigned/signed variants) used widely.

1) File Envelope (All Fast modes)
- Magic: 4 ASCII bytes `loro`.
- Checksum: 16 bytes in header, semantics depend on mode:
  - FastSnapshot and FastUpdates: last 4 bytes store `xxhash32(body, seed="LORO")` in little-endian; the leading 12 bytes are zeroed.
  - (Legacy modes used MD5 across all 16 bytes; not used here.)
- Mode: 2 bytes big-endian (`0x0003` FastSnapshot, `0x0004` FastUpdates).
- Body: Mode-specific payload bytes immediately follow.

2) Shared Binary Conventions
- Integers: LEB128 varints are used for most variable-length integers (counts, lengths, deltas). Both unsigned and signed forms appear (e.g., counters use unsigned; some deltas use signed).
- Endianness:
  - Header mode uses big-endian u16.
  - Segment lengths in Fast Snapshot use little-endian u32.
  - f64 values are stored as 8-byte big-endian.
- Strings/Binary:
  - Length-prefixed via unsigned LEB128, followed by raw bytes (UTF-8 for strings).
  - Keys may be deduplicated/referenced via arenas/dictionaries.
- Safety Limits:
  - Collections and decoded sizes are capped in decoders to prevent DoS (e.g., MAX_COLLECTION_SIZE, MAX_DECODED_SIZE) — implementations should enforce reasonable limits.

3) KV Store Framing (used by oplog store and state store)
- Export format (prefix-compressed keys):
  - For the first key: `[uLEB key_len][key_bytes]`.
  - For subsequent keys: `[u8 common_prefix_len][uLEB suffix_len][suffix_bytes]`.
  - Each value: `[uLEB value_len][value_bytes]`.
- Oplog KV keys include special entries like `vv` (VersionVector), `fr` (Frontiers), and block keys: 12 bytes `peer_id (8) + counter (4)`. State store keys are encoded `ContainerID`s.
- Import restores entries by reversing the above. Implementations must preserve key ordering for prefix compression to be valid.

4) Fast Snapshot Format
- Body layout (little-endian u32 lengths):
  - `[u32 len][oplog_bytes]`
  - `[u32 len][state_bytes_or_E]` where `E` (single byte 0x45) is a sentinel for “absent state”
  - `[u32 len][shallow_root_state_bytes]` (empty if no GC baseline)
- Semantics:
  - `oplog_bytes`: Encoded change store (KV store export). Contains all changes, vv and frontiers as of export.
  - `state_bytes_or_E`:
    - Present: Encoded state store covering all alive containers at the latest version.
    - Absent (sentinel `E`): Importer must compute latest state by replaying changes on top of the baseline (see below).
  - `shallow_root_state_bytes` (GC baseline):
    - Empty => Full snapshot: import `state_bytes` directly.
    - Non-empty => GC snapshot: baseline state at `shallow_root_frontiers`, and either:
      - If `state_bytes` present: import baseline and merge `state_bytes` to reconstruct latest state without replay.
      - If `state_bytes` absent: importer derives latest by starting from baseline and replaying the needed changes.
- Import flow (high level):
  - Decode `oplog_bytes` into the change store; set VV/frontiers accordingly.
  - If GC baseline exists: load baseline first; then either merge `state_bytes`, or if absent, compute by replay.
  - If no GC baseline: load `state_bytes` if present; else compute by replay.

5) Fast Updates Format
- Body is a concatenation of “change blocks”:
  - `[uLEB block_len][block_bytes]` repeated until EOF.
- Each `block_bytes` is a serialized “encoded block” that includes:
  - Summary ranges: starting counter/length and starting lamport/length (allow quick range checks).
  - Change header and metadata: peer registers, per-change counters/lengths, per-change deps, lamports, timestamps (delta-of-delta), commit messages (lengths via RLE and concatenated bytes).
  - Operation columns:
    - Dictionaries/arenas: container IDs, string keys, positions, tree IDs, peers.
    - Columnar ops: sequences of per-op fields encoded via serde_columnar strategies (RLE, DeltaRle, DeltaOfDelta, etc.)
    - Delete-start IDs (optional) encoded columnar if present.
    - Value bytes pool: compact value stream referenced by ops (see “Value Encoding”).
- Block decoding (conceptual):
  - Read `block_len`, slice `block_bytes`.
  - Parse header to reconstruct change boundaries: peer, per-change counter extents, lamports, dependency frontiers, timestamps, and commit messages.
  - Decode arenas (peers, container IDs, keys, tree IDs, positions) and op columns.
  - For each op, decode its value and apply to the corresponding container/change via the reconstructed IDs and arenas.
- Ordering and causality:
  - Blocks contain changes for a single peer, contiguous in (counter, lamport) ranges.
  - Export guarantees causal order within and across blocks; import sorts changes by lamport before applying to the DAG.

6) Value Encoding (used by Fast Updates blocks)
- Value type tag (1 byte), then type-specific content; key details:
  - Null/True/False: tag only.
  - I64: signed LEB128.
  - F64: 8-byte big-endian.
  - Str/Binary: `[uLEB len][bytes]`.
  - Container reference: stores container type index; container IDs themselves are provided by the ContainerID arena.
  - Composite LoroValue (Map/List):
    - List: `[uLEB len]` followed by nested value entries.
    - Map: `[uLEB len]` followed by repeated `[uLEB key_index][value]` pairs; keys are resolved from the key arena.
  - CRDT Operation payloads:
    - MarkStart: `[u8 info][uLEB len][uLEB key_index][value]`.
    - ListMove/ListSet: small tuples of LEB128 numbers identifying from/to, peer indices, and lamport.
    - TreeMove/RawTreeMove: indices into peer and position arenas plus flags/counters indicating parent/subject.
- Future-proofing:
  - Unknown/Future kinds carry an extra tag bit and store raw bytes; decoders must carry through unrecognized kinds losslessly.

7) Arenas and Registers (dictionaries)
- Purpose: deduplicate repeated data across a block and reduce size.
- Peers (PeerIdArena): `[uLEB count][N * 8-byte peer_id]`.
- Containers (ContainerArena): Each entry encodes whether root, type, peer index, and key index or counter; decoders expand to `ContainerID`.
- Keys (KeyArena): Columnar array of UTF-8 strings.
- Tree IDs (TreeIDArena): Peer index + delta-encoded counters.
- Positions (PositionArena): For fractional indices, encodes per-position “delta” as common-prefix length + suffix; decoders reconstruct full bytes by prefix restore.
- Deps (DepsArena): Per-change dependency peers and counters encoded columnarly.

8) Shallow Snapshot Format
- Purpose: Serialize a compact snapshot “since” a shallow frontier to trim old history and optionally avoid replay on import.
- Body (same Fast Snapshot framing; see section 4):
  - `oplog_bytes`: kv-store export of changes from `start_vv` (derived from the chosen shallow frontier) to latest; includes vv/frontiers metadata of the export window.
  - `shallow_root_state_bytes`: kv-store export of the baseline state at `start_frontiers`, including a `fr` entry storing that frontier.
  - `state_bytes` (optional): If the number of ops since `start_frontiers` exceeds a threshold, a delta state is exported so the importer can avoid replay.
- Export steps (high level):
  - Choose `start_frontiers` as the LCA of the requested target and latest (with a special adjustment for style start/end pairing in rich text).
  - Export change-store from `start_vv` to latest.
  - Checkout to `start_frontiers`, flush state store, copy live containers into `shallow_root_state_bytes` (record the `fr` key with the baseline).
  - If too many ops since baseline, also export `state_bytes` as a delta relative to baseline; otherwise omit and let importer replay.
- Import mirrors Fast Snapshot GC behavior: load baseline, then either merge `state_bytes` or replay the appended updates.

9) Decoding/Encoding Flows (Integration)
- Fast Snapshot export: header → oplog export → state export (and optional GC baseline) → finalize header checksum.
- Fast Snapshot import: parse header → validate checksum → decode oplog → decode state and (optional) baseline → if needed, compute latest state.
- Fast Updates export: scan delta vs input VV → build blocks → write `[uLEB len][block]`…
- Fast Updates import: iterate blocks → decode, sort by lamport → feed into DAG → apply or queue pending if deps missing.

10) LEB128 Primer
- Unsigned LEB128: encode integers in base-128, least significant 7 bits per byte; set MSB=1 to continue, MSB=0 to end.
- Signed LEB128: similar layout but with sign extension for negative values (no ZigZag used here).
- Usage in Loro: lengths/counts (unsigned), deltas (often signed), and many small integers across headers and value payloads.

11) Compatibility and Checksums
- Header checksum:
  - FastSnapshot/FastUpdates: `xxhash32` of the body (seed `LORO`), stored in the last 4 header checksum bytes (little-endian).
- Mode bytes: 2-byte big-endian.
- Be mindful that legacy blobs may use MD5 across 16 checksum bytes and different mode IDs.

12) Practical Notes
- Keys and Containers in State Store:
  - Keys are UTF-8 strings; containers are identified by either root name+type or peer+counter+type.
- Replay vs Merge on Shallow Import:
  - Importers may compute latest state via replay if `state_bytes` is omitted; otherwise merge baseline + delta.
- Bounded Decoding:
  - Always enforce collection/size caps and validate indices when dereferencing arenas.

13) Implementation Checklist
- Envelope: parse header, validate checksum, route by mode.
- KV Store: implement prefix-compressed key export/import.
- Fast Snapshot:
  - Segment decode, GC baseline handling, optional replay.
- Fast Updates:
  - Block loop, header/meta decode, arenas, op/value decode, lamport ordering.
- Values:
  - Support all listed kinds and nested LoroValue, including future/unknown kinds.

14) Open TODOs / Further Research
- Block container encoding details: The top-level block uses postcard serialization in Rust. For non-Rust implementations, either:
  - Implement a compatible postcard decoder, or
  - Specify a stable, explicit byte layout for `EncodedBlock` (field order, varint forms, container framing) to decouple from postcard.
- [x] Write a precise, postcard-free schema for `EncodedBlock`.
- [ ] Produce a normative per-field table of serde_columnar strategies (RLE, DeltaRle, DeltaOfDelta) used for ops/deps.
- [ ] Document rich-text StyleStart/StyleEnd pairing and shallow frontier rules with examples.
- [ ] Finalize recommended MAX_* limits (size caps) for interoperability and security across languages.
- [ ] Produce an endianness matrix table and tests.

References (code pointers)
- Header/modes/checksum: `crates/loro-internal/src/encoding.rs`
- Fast Snapshot: `crates/loro-internal/src/encoding/fast_snapshot.rs`
- Shallow Snapshot: `crates/loro-internal/src/encoding/shallow_snapshot.rs`
- KV Store framing: `crates/loro-internal/src/kv_store.rs`
- Fast Updates block encode/decode: `crates/loro-internal/src/oplog/change_store/{block_encode.rs, block_meta_encode.rs, change_store.rs}`
- Value encoding: `crates/loro-internal/src/encoding/value.rs`
