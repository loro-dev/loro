# Loro current binary encoding

Verified against code 2026-07-17 at commit
`fd5a1fdab79142302f0c0fbceb8807128ec6d9cd`.

This is the normative wire-format reference for the binary formats currently
written by Loro:

- ordinary snapshots (`EncodeMode::FastSnapshot`, value `3`),
- shallow snapshots (`EncodeMode::FastSnapshot`, value `3`), and
- updates (`EncodeMode::FastUpdates`, value `4`).

Legacy top-level modes and JSON updates are intentionally out of scope. The
`StateOnly` and `SnapshotAt` export APIs reuse the snapshot wire format, but are
not separate on-wire formats and are not specified here. A decoder must use the
bytes, not the API name that produced them.

Every normative section below links both the writer and the reader. A source
link is pinned by line for this verified commit and also names the relevant
symbol so that it remains searchable after lines move. Third-party codecs are
identified by the exact version in `Cargo.lock`.

## 1. Notation and dependency versions

| Notation | Meaning |
|---|---|
| `u8` | one unsigned byte |
| `u16le`, `u32le`, `u64le` | fixed-width unsigned little-endian integer |
| `u16be`, `u32be`, `u64be` | fixed-width unsigned big-endian integer |
| `i32le`, `i32be` | two's-complement fixed-width signed integer |
| `uleb` | unsigned LEB128 as used by the `leb128` crate |
| `sleb` | signed LEB128 with sign extension as used by the `leb128` crate |
| `pvar(T)` | postcard encoding of `T`; unsigned integers use unsigned LEB128, signed integers use zigzag followed by unsigned LEB128 |
| `bytes` | `uleb(byte_length)` followed by that many bytes |
| `postcard(T)` | postcard 1.1.3 serialization of `T` |
| `columnar(T)` | serde_columnar 0.3.14 serialization of `T`, built on postcard |

The locked implementations are postcard 1.1.3, serde_columnar 0.3.14,
lz4_flex 0.11.5, and xxhash-rust 0.8.15. See
[`Cargo.lock`](../Cargo.lock#L1938-L1944),
[`Cargo.lock`](../Cargo.lock#L2304-L2313),
[`Cargo.lock`](../Cargo.lock#L2704-L2713), and
[`Cargo.lock`](../Cargo.lock#L3738-L3742).

For source-level dependency tracing, the published crate archives record these
VCS revisions in `.cargo_vcs_info.json`: postcard
`718aa6a6850456017c19eeff67303c633f875736`, serde_columnar
`06663cfc569c6c770b28688f015f8a87fc04e156`, serde_columnar_derive
`82588626dfa4332971c1e98f2839eb6d1c3dae1d`, lz4_flex
`4c4ba15a4ce3ba3f0125177a0e4bba39f3d3a1e7`, and xxhash-rust
`7026cd705195f502283f97aafc9ea41930099c68`.

Do not interchange `sleb` and postcard signed integers. For example, `-1` is
`7f` in `sleb`, but `01` in postcard zigzag encoding.
Postcard integer, zigzag, sequence-length, and little-endian float behavior is
implemented in
`postcard-1.1.3/src/{ser/serializer.rs,de/deserializer.rs}` at the pinned source
revision above.

## 2. Document envelope

Every current binary blob starts with a 22-byte envelope.

| Offset | Size | Field | Encoding and invariant |
|---:|---:|---|---|
| 0 | 4 | magic | ASCII `loro` (`6c 6f 72 6f`) |
| 4 | 12 | checksum prefix | the current encoder writes zero; the current mode-3/mode-4 decoder does not interpret these bytes |
| 16 | 4 | checksum | `u32le xxHash32(blob[20..], seed = 0x4f524f4c)` |
| 20 | 2 | mode | `u16be`; `3` is FastSnapshot and `4` is FastUpdates |
| 22 | remaining | body | snapshot body or updates body |

The checksum includes the two mode bytes. It does not start at offset 22. The
12-byte checksum prefix is zero in canonical output, but is not currently a
validation condition for modes 3 and 4.

Writer: [`encoding.rs::encode_with`](../crates/loro-internal/src/encoding.rs#L440-L459),
[`EncodeMode::to_bytes`](../crates/loro-internal/src/encoding.rs#L204-L208).
Reader: [`encoding.rs::parse_header_and_body`](../crates/loro-internal/src/encoding.rs#L331-L384),
[`ParsedHeaderAndBody::check_checksum`](../crates/loro-internal/src/encoding.rs#L300-L328).
The hash algorithm and all three checksum locations are specified in
[encoding-xxhash32.md](./encoding-xxhash32.md).

## 3. FastSnapshot body

Mode 3 has exactly three length-prefixed sections and no trailing bytes:

```text
u32le oplog_len
u8     oplog_bytes[oplog_len]
u32le state_len
u8     state_bytes[state_len]
u32le shallow_root_state_len
u8     shallow_root_state_bytes[shallow_root_state_len]
EOF
```

All lengths count bytes, not entries. The decoder rejects a missing length,
any length beyond the remaining input, and bytes after the third section.

`state_bytes == [0x45]` (ASCII `E`) is a sentinel for **section omitted**. It
does not mean an empty document or an empty KV store. An empty KV store is zero
bytes and therefore has `state_len == 0`. Current writers emit `E` only for a
shallow snapshot that omits the end-state overlay and expects the importer to
replay retained history from the shallow-root state.

Writer and sentinel:
[`fast_snapshot.rs::Snapshot`, `_encode_snapshot`, `EMPTY_MARK`](../crates/loro-internal/src/encoding/fast_snapshot.rs#L16-L45).
Reader and exact EOF check:
[`fast_snapshot.rs::_decode_snapshot_bytes`](../crates/loro-internal/src/encoding/fast_snapshot.rs#L47-L95).
Empty KV representation:
[`mem_store.rs::MemKvStore::export_all`](../crates/kv-store/src/mem_store.rs#L210-L250).

### 3.1 Section meaning by snapshot kind

| Kind written by the current encoder | `oplog_bytes` | `state_bytes` | `shallow_root_state_bytes` |
|---|---|---|---|
| ordinary snapshot from a non-shallow document | complete ChangeStore through the OpLog latest version | complete container state at the OpLog latest version; zero bytes is a valid empty state | zero bytes |
| shallow snapshot | partial ChangeStore from actual shallow root `S` through OpLog latest | `E`, or a latest-state overlay relative to the root state | state at `S`, including state-store key `b"fr" = encode(S)` |
| `Snapshot` requested from a document that is already shallow | same as shallow snapshot; unavailable history cannot be recreated | same as shallow snapshot | same as shallow snapshot |

The mode field alone cannot distinguish ordinary and shallow snapshots. The
current metadata reader classifies a mode-3 blob as shallow exactly when the
third section length is nonzero. There is no on-wire subtype for the export API
variant.

Writers:
[`fast_snapshot.rs::encode_snapshot_inner`](../crates/loro-internal/src/encoding/fast_snapshot.rs#L277-L323),
[`shallow_snapshot.rs::export_shallow_snapshot_inner`](../crates/loro-internal/src/encoding/shallow_snapshot.rs#L32-L187).
Classification:
[`fast_snapshot.rs::_decode_snapshot_meta_partial`](../crates/loro-internal/src/encoding/fast_snapshot.rs#L97-L126),
[`decode_snapshot_blob_meta`](../crates/loro-internal/src/encoding/fast_snapshot.rs#L404-L426).

### 3.2 Ordinary snapshot selection

For a non-shallow source document, `Snapshot` writes:

1. the complete ChangeStore SSTable;
2. a complete state SSTable at the OpLog latest frontier; and
3. an empty third section.

If the document is checked out to an older version, export temporarily obtains
the latest state for section 2. "Snapshot state" therefore means the state at
the OpLog end, not necessarily the state visible at the caller's current
checkout.

Writer:
[`fast_snapshot.rs::encode_snapshot_inner`](../crates/loro-internal/src/encoding/fast_snapshot.rs#L277-L323).
ChangeStore writer:
[`change_store.rs::ChangeStore::encode_all`](../crates/loro-internal/src/oplog/change_store.rs#L160-L164),
[`flush_and_compact`](../crates/loro-internal/src/oplog/change_store.rs#L729-L766).
State writer:
[`container_store.rs::ContainerStore::encode`](../crates/loro-internal/src/state/container_store.rs#L173-L175),
[`inner_store.rs::InnerStore::encode`](../crates/loro-internal/src/state/container_store/inner_store.rs#L216-L219).

### 3.3 Shallow-root selection

Let `F` be the frontiers requested by the caller. The wire root is an actual
frontier set `S = calc_shallow_doc_start(F)`, which can differ from `F`:

1. multiple frontier IDs are reduced by pairwise greatest-common-ancestor
   calculations until one frontier (or the empty frontier) remains; if a
   reduction round makes no progress, the algorithm falls back to the empty
   frontier before the existing-root clamp;
2. if the selected ID is a rich-text `StyleStart`, `S` advances to the adjacent
   `StyleEnd`, so the pair is not split by the shallow boundary; and
3. an already-shallow document clamps `S` to its existing shallow root, because
   history before that point is unavailable.

The public export path first rejects an unreachable request. A decoder does not
repeat this selection; it consumes the `sv`, `sf`, and root-state `fr` values
already present in the blob.

Selection:
[`shallow_snapshot.rs::calc_shallow_doc_start`, `clamp_to_shallow_root`](../crates/loro-internal/src/encoding/shallow_snapshot.rs#L312-L383).
Reachability check:
[`encoding.rs::check_target_version_reachable`](../crates/loro-internal/src/encoding.rs#L420-L427).

### 3.4 Shallow ChangeStore range

The shallow ChangeStore contains four metadata concepts:

- `sv`: the start VersionVector;
- `sf`: the actual shallow-root frontiers `S`;
- `vv`: the end VersionVector (OpLog latest); and
- `fr`: the end frontiers (OpLog latest).

VersionVector counters are exclusive end counters. The exporter first computes
the VersionVector at `S`, then replaces each frontier peer's end with that
frontier ID's own counter. This makes `sv` describe the point immediately
before the boundary operation on those peers, so the encoded change blocks
retain the boundary operation as a history anchor. The encoded interval is
therefore not simply "operations after the caller's `F`".

The current shallow writer always stores both `sv` and `sf`, including when
`S` is empty. An empty VersionVector map and an empty Frontiers vector each
postcard-encode as the single byte `00`, so that case contains `sv = [00]` and
`sf = [00]`; neither value is a zero-byte KV value. The importer also accepts
an absent or zero-byte `sv` for compatibility. After decoding, it treats a
semantically empty VersionVector as no shallow DAG start.

Range construction:
[`shallow_snapshot.rs::export_shallow_snapshot_inner`](../crates/loro-internal/src/encoding/shallow_snapshot.rs#L32-L76).
ChangeStore range writer:
[`change_store.rs::ChangeStore::export_from`, `encode_from`](../crates/loro-internal/src/oplog/change_store.rs#L160-L252).
ChangeStore reader:
[`change_store.rs::ChangeStore::import_all`](../crates/loro-internal/src/oplog/change_store.rs#L633-L725).

### 3.5 Shallow state root and overlay

`shallow_root_state_bytes` is a state SSTable for `S`. In addition to container
entries, it contains:

```text
key   = 66 72                 # ASCII "fr"
value = Frontiers::encode(S)  # postcard Vec<ID>
```

`state_bytes` has one of two forms:

- `E`: no end-state overlay; import replays retained changes from `S` to the
  ChangeStore end; or
- a KV overlay encoding: entries whose bytes differ from the root state,
  filtered to the retained container set. It is zero bytes when the overlay has
  no entries and otherwise is an SSTable. Import applies it after the root
  SSTable, so an overlay value with the same key replaces the root value.

The production exporter currently chooses `E` when the retained tail has at
most 256 operation atoms and may choose an overlay above that threshold. Tests
compile with a threshold of 16. This is an encoding heuristic, **not** a wire
invariant: decoders must inspect the section bytes and must not reproduce the
threshold decision.

Root/overlay writer:
[`shallow_snapshot.rs::export_shallow_snapshot_inner`](../crates/loro-internal/src/encoding/shallow_snapshot.rs#L68-L184).
Threshold definition:
[`shallow_snapshot.rs::MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE`](../crates/loro-internal/src/encoding/shallow_snapshot.rs#L16-L19).
Overlay construction:
[`kv_wrapper.rs::remove_same`, `retain_keys`](../crates/loro-internal/src/utils/kv_wrapper.rs#L94-L142).
Reader and overlay order:
[`fast_snapshot.rs::decode_snapshot_inner`](../crates/loro-internal/src/encoding/fast_snapshot.rs#L168-L258),
[`inner_store.rs::decode_twice`](../crates/loro-internal/src/state/container_store/inner_store.rs#L286-L317).

Normal snapshots can preserve opaque state bytes for lazily loaded unknown
container types. Shallow-export behavior is path-dependent:

- when it rebuilds the root at `S`, it rejects an unknown materialized live
  root container;
- when it reuses a cached root and builds a latest-state overlay, it rejects an
  unknown retained root key; but
- when it reuses that cached root with the replay-only `E` fast path, it returns
  the existing root bytes without an unknown-kind check, so a lazy root unknown
  can survive.

The later scan also does not repeat the check for containers introduced after
`S`. Such a post-root unknown can therefore be carried either by retained
operations in the `E` form or by raw/lazy wrapper bytes selected into an
overlay SSTable. Whether re-encoding preserves its opaque payload then depends
on whether the wrapper remains lazy; section 10 of
[encoding-container-states.md](./encoding-container-states.md#10-unknown-state)
specifies that distinction. This is exporter behavior, not a new wire tag.

Path-specific fast path and checks:
[`export_shallow_snapshot_inner`](../crates/loro-internal/src/encoding/shallow_snapshot.rs#L68-L169),
[`shallow_snapshot.rs::has_unknown_container`, `has_unknown_container_key`](../crates/loro-internal/src/encoding/shallow_snapshot.rs#L189-L196),
with state-only checks in
[`export_state_only_snapshot`](../crates/loro-internal/src/encoding/shallow_snapshot.rs#L198-L264).

### 3.6 Snapshot import into empty and non-empty documents

The state sections initialize a document only when
`LoroDoc::can_reset_with_snapshot()` is true. Direct initialization decodes the
ChangeStore and the state sections atomically. If `state_bytes` is `E`, a
shallow snapshot initializes at `S` and checks out to the retained history's
latest version.

When importing the same mode-3 blob into a non-empty or detached document, Loro
uses only the encoded ChangeStore as incoming changes; the two state sections
do not overwrite the existing materialized state. The ChangeStore is the first
snapshot section, so this path first reads its `u32le` length and ignores the
remaining snapshot sections.

Dispatch:
[`loro.rs::LoroDoc::_import_with`](../crates/loro-internal/src/loro.rs#L582-L638).
Direct initialization:
[`fast_snapshot.rs::decode_snapshot_inner`](../crates/loro-internal/src/encoding/fast_snapshot.rs#L168-L258).
Change-only path:
[`fast_snapshot.rs::decode_oplog`](../crates/loro-internal/src/encoding/fast_snapshot.rs#L326-L344).

## 4. SSTable used by snapshot sections

Except for the one-byte `E` sentinel allowed only in shallow `state_bytes`,
every non-empty `oplog_bytes`, `state_bytes`, and
`shallow_root_state_bytes` section is a `loro-kv-store` SSTable. A zero-length
section represents an empty KV store and has no SSTable magic, schema byte,
metadata, or footer.

### 4.1 Non-empty SSTable layout

```text
offset  size      field
0       4         ASCII "LORO"
4       1         schema version = 0
5       variable  block 0
...               further blocks
M       variable  block metadata
EOF-4   4         u32le M, absolute offset of block metadata
```

The metadata offset must point before the four-byte footer. A current SSTable
must have at least one block.

Writer:
[`sstable.rs::SsTableBuilder::new`, `build`](../crates/kv-store/src/sstable.rs#L164-L307).
Reader:
[`sstable.rs::SsTable::import_all_with`](../crates/kv-store/src/sstable.rs#L369-L429),
[`validate_block_ranges`](../crates/kv-store/src/sstable.rs#L434-L451).

### 4.2 Block metadata

```text
u32le block_count
repeat block_count times:
    u32le block_offset
    u16le first_key_len
    u8    first_key[first_key_len]
    u8    flags
    if (flags & 0x80) == 0:
        u16le last_key_len
        u8    last_key[last_key_len]
u32le metadata_checksum
```

`flags & 0x80` is `is_large`; `flags & 0x7f` is the compression type (`0 =
none`, `1 = LZ4 frame`). A large block represents exactly one key, so it omits
`last_key`. The metadata checksum is xxHash32 with Loro seed over the metadata
entries only: it excludes `block_count` and excludes the checksum itself.

The current reader caps `block_count` at 10,000,000 and requires it to be
nonzero for a non-empty SSTable. Block offsets are absolute, strictly advance
from at least byte 5, end no later than the metadata offset, and give every
stored block at least four bytes for its checksum. A checked import also
reconstructs block keys, requires metadata `last_key` to match, and requires
strict key order across blocks.

Writer and checksum range:
[`sstable.rs::BlockMeta::encode_meta`](../crates/kv-store/src/sstable.rs#L41-L89).
Reader:
[`sstable.rs::BlockMeta::decode_meta`](../crates/kv-store/src/sstable.rs#L91-L147).
Range/key validation:
[`validate_block_ranges`, `validate_blocks`](../crates/kv-store/src/sstable.rs#L434-L488).
Compression tag:
[`compress.rs::CompressionType`](../crates/kv-store/src/compress.rs#L7-L39).

### 4.3 Normal block

Before optional compression, a normal block is:

```text
u8     entry_data[entry_data_len]
u16le offsets[entry_count]       # offsets[0] is 0
u16le entry_count                # nonzero
```

That entire body is either stored directly or encoded as one LZ4 frame. The
stored payload is then followed by:

```text
u32le xxHash32(stored_payload, LORO_SEED)
```

`offsets[i]` points to entry `i` inside the uncompressed `entry_data`. The end
of an entry is `offsets[i+1]`, or `entry_data_len` for the last entry.

The first entry's key is `BlockMeta.first_key`, so its entry bytes are only its
value. Every later entry is:

```text
u8    common_prefix_len          # prefix of BlockMeta.first_key
u16le key_suffix_len
u8    key_suffix[key_suffix_len]
u8    value[remaining_entry_len]
```

There is no value length inside an entry; the offsets delimit it. Keys are
strictly increasing byte strings. Prefix length is limited to 255 bytes; the
encoder may use a shorter prefix than the maximum and the reconstructed key is
still authoritative.

Writer:
[`block.rs::BlockBuilder::add`](../crates/kv-store/src/block.rs#L351-L432),
[`NormalBlock::encode`](../crates/kv-store/src/block.rs#L77-L118).
Reader and validation:
[`block.rs::NormalBlock::decode`, `validate_decoded_data`](../crates/kv-store/src/block.rs#L120-L228).

### 4.4 Large-value block

A value becomes a one-entry large block when it is the first entry of a fresh
block and its value length exceeds the configured block size (4 KiB for these
stores) or exceeds `u16::MAX`. The key is present only in block metadata. The
stored block is:

```text
u8    value_or_lz4_frame[variable]
u32le xxHash32(previous_bytes, LORO_SEED)
```

Writer and fallback:
[`block.rs::BlockBuilder::add`](../crates/kv-store/src/block.rs#L373-L390),
[`LargeValueBlock::encode`](../crates/kv-store/src/block.rs#L18-L58).
Reader:
[`LargeValueBlock::decode`](../crates/kv-store/src/block.rs#L60-L70).
Default block size:
[`mem_store.rs::MemKvStore::DEFAULT_BLOCK_SIZE`](../crates/kv-store/src/mem_store.rs#L60-L71).

### 4.5 Compression choice and block checksums

The store asks for LZ4 by default. Each block is first framed with LZ4; if the
complete frame is larger than the uncompressed block body, the writer discards
it and stores the body with compression type 0. Equal length remains LZ4. The
checksum always covers the actually stored payload and is never compressed.

Loro snapshot import has already verified the document checksum and therefore
uses the SSTable path that skips eager per-block checksum verification. It
still parses and verifies block metadata. Standalone checked SSTable import can
verify each block checksum. This optimization does not change the wire bytes.

Writers:
[`block.rs::NormalBlock::encode`](../crates/kv-store/src/block.rs#L93-L118),
[`LargeValueBlock::encode`](../crates/kv-store/src/block.rs#L39-L58).
Import modes:
[`sstable.rs::import_all_unchecked`, `import_all_with`](../crates/kv-store/src/sstable.rs#L360-L429),
[`sstable.rs::check_block_checksum`](../crates/kv-store/src/sstable.rs#L488-L518).
LZ4 details: [encoding-lz4.md](./encoding-lz4.md).

## 5. ChangeStore SSTable schema

`oplog_bytes` is an SSTable with the following logical entries.

| Key bytes | Value | Meaning |
|---|---|---|
| ASCII `vv` | `VersionVector::encode()` | exclusive end VersionVector |
| ASCII `fr` | `Frontiers::encode()` | end frontiers |
| ASCII `sv` | `VersionVector::encode()` | shallow exclusive start VV; always present in a current shallow export (`00` for an empty map), absent for full history |
| ASCII `sf` | `Frontiers::encode()` | actual shallow-root frontiers `S`; always present in a current shallow export (`00` for an empty vector), absent for full history |
| 12-byte `ID::to_bytes()` | postcard change block | block beginning at that `(peer, counter)` |

The same two bytes `fr` are also used as a key in the separate shallow-root
**state** SSTable. They do not conflict: ChangeStore `fr` is the history end,
while root-state `fr` records the shallow root.

Key declarations and invariants:
[`change_store.rs::START_VV_KEY`, `START_FRONTIERS_KEY`, `VV_KEY`, `FRONTIERS_KEY`](../crates/loro-internal/src/oplog/change_store.rs#L134-L137),
[`ChangeStore` encoding schema](../crates/loro-internal/src/oplog/change_store.rs#L39-L60).
Writer:
[`ChangeStore::encode_from`, `flush_and_compact`](../crates/loro-internal/src/oplog/change_store.rs#L235-L252),
[`change_store.rs`](../crates/loro-internal/src/oplog/change_store.rs#L729-L766).
Reader:
[`ChangeStore::import_all`](../crates/loro-internal/src/oplog/change_store.rs#L633-L725).

### 5.1 Change block key

```text
u64be peer
i32be counter_start
```

This fixed 12-byte key is `ID::to_bytes()`. It is unrelated to postcard's
serialization of `ID`, which uses varints.

Writer/reader:
[`loro-common::ID::to_bytes`, `from_bytes`](../crates/loro-common/src/lib.rs#L43-L61).

### 5.2 VersionVector value

`VersionVector` is a postcard-encoded `FxHashMap<PeerID, Counter>`:

```text
pvar(entry_count)
repeat entry_count times:
    pvar(u64 peer)
    pvar(i32 exclusive_counter)  # zigzag + unsigned LEB128
```

Map entry order is not canonical. Decoders must treat it as an unordered map,
and two semantically equal VersionVectors need not have identical bytes.

Writer/reader:
[`version.rs::VersionVector::encode`, `decode`](../crates/loro-internal/src/version.rs#L960-L970),
backing type:
[`version.rs::VersionVector`](../crates/loro-internal/src/version.rs#L19-L30).

### 5.3 Frontiers value

`Frontiers::encode()` collects IDs, sorts them by Rust `ID` order, and postcard
serializes `Vec<ID>`:

```text
pvar(id_count)
repeat id_count times:
    pvar(u64 peer)
    pvar(i32 counter)  # zigzag + unsigned LEB128
```

Canonical writers sort; readers accept any vector order and construct the set
semantics of `Frontiers`.

Writer/reader:
[`frontiers.rs::Frontiers::encode`, `decode`](../crates/loro-internal/src/version/frontiers.rs#L219-L232).
`ID` field order:
[`loro-common::ID`](../crates/loro-common/src/lib.rs#L35-L41).

## 6. FastUpdates body

Mode 4 is a concatenation with no block count and no terminator:

```text
repeat until EOF:
    uleb block_len
    u8   postcard_change_block[block_len]
```

An empty body is a valid empty updates blob. Change blocks are not SSTable
blocks and are not LZ4-compressed. The 4 KiB ChangeStore constant is an
estimated-storage-size split target, not an encoded size limit and not a promise
about compressed size. A block contains one or more changes; a single-change
block is valid. Mode 4 has no per-block checksum: integrity is supplied only by
the document-envelope checksum.

Writer:
[`fast_snapshot.rs::encode_updates`, `encode_updates_in_range`](../crates/loro-internal/src/encoding/fast_snapshot.rs#L346-L360),
[`change_store.rs::encode_blocks_in_store`](../crates/loro-internal/src/oplog/change_store.rs#L614-L625).
Reader and length checks:
[`fast_snapshot.rs::decode_updates`](../crates/loro-internal/src/encoding/fast_snapshot.rs#L360-L388).
Split heuristic:
[`change_store.rs::MAX_BLOCK_SIZE`](../crates/loro-internal/src/oplog/change_store.rs#L35-L38),
[`insert_change_inner`](../crates/loro-internal/src/oplog/change_store.rs#L808-L866).

The updates APIs differ only in which changes are selected:
`Updates { from }` emits ranges after a VersionVector (clamped to an existing
shallow root), while `UpdatesInRange` emits the normalized requested ID spans.
Both produce the exact body above.

On import, each decoded block is filtered against the receiver's current
VersionVector. A change entirely before the receiver's exclusive counter end is
dropped; a change crossing that boundary is sliced at the boundary. The
remaining changes from every block are then sorted by starting Lamport value
before application. Block order in the byte stream is therefore not application
order. These selection and ordering rules are API behavior around the wire
format; they do not add fields to it.

Selection:
[`ChangeStore::export_blocks_from`](../crates/loro-internal/src/oplog/change_store.rs#L543-L577),
[`export_blocks_in_range`](../crates/loro-internal/src/oplog/change_store.rs#L203-L233).
Import filtering and ordering:
[`ChangeStore::decode_block_bytes`](../crates/loro-internal/src/oplog/change_store.rs#L278-L300),
[`fast_snapshot.rs::decode_updates`](../crates/loro-internal/src/encoding/fast_snapshot.rs#L360-L388).

## 7. Postcard change block

Every current block is a postcard struct serialized field-by-field in this
exact order. Postcard structs do not write field names or a field count.

```text
pvar(u32 counter_start)
pvar(u32 counter_len)
pvar(u32 lamport_start)
pvar(u32 lamport_len)
pvar(u32 n_changes)
bytes header
bytes change_meta
bytes cids
bytes keys
bytes positions
bytes ops
bytes delete_start_ids
bytes values
```

All changes in a block belong to one peer and are counter-contiguous. Let
`first` and `last` be the first and last changes. The canonical writer sets:

```text
counter_start = first.id.counter
counter_len   = last.ctr_end() - counter_start
lamport_start = first.lamport
lamport_len   = last.lamport_end() - lamport_start
n_changes     = number of changes, which is at least 1
```

The first block for a peer may start above zero after history trimming.
`counter_len` and `lamport_len` are spans, not inclusive end values. There is no
encoded version prefix before `counter_start` in blocks written by
`encode_block`.

Writer and field order:
[`block_encode.rs::EncodedBlock`, `encode_block`](../crates/loro-internal/src/oplog/change_store/block_encode.rs#L91-L279).
Reader:
[`block_encode.rs::decode_header`](../crates/loro-internal/src/oplog/change_store/block_encode.rs#L388-L413),
[`block_encode.rs::decode_block`](../crates/loro-internal/src/oplog/change_store/block_encode.rs#L535-L705).
Block invariants:
[`change_store.rs::ChangeStore`](../crates/loro-internal/src/oplog/change_store.rs#L39-L60).

### 7.1 `header`

Let `N = n_changes`, `D = total number of non-self dependencies`, and let
`peers[0]` be the block's own peer. The peer table must be non-empty.

```text
uleb peer_count
repeat peer_count times:
    u64le peer
repeat N-1 times:
    uleb change_atom_len
BoolRle[N]          dep_on_self
AnyRle<usize>[N]    non_self_dep_count
AnyRle<u32>[D]      dependency_peer_index
DeltaOfDelta[D]     dependency_counter
DeltaOfDelta[N-1]   change_lamport
EOF
```

The final change atom length is
`counter_len - sum(first N-1 lengths)`. A true `dep_on_self[i]` reconstructs
dependency `(peer[0], change_start_counter - 1)`; the parallel dependency
count stores only non-self dependencies. Peer indices are zero-based.

The last change's starting Lamport value is inferred from
`lamport_start + lamport_len - last_change_atom_len`; only the first `N-1`
starting Lamport values are in the DeltaOfDelta stream.

The peer-table order is first registration order, with the block peer
registered before any dependency, container, delete, movable-list, or tree
peer. This makes index zero the block peer in canonical bytes.

Writer:
[`block_meta_encode.rs::encode_changes`](../crates/loro-internal/src/oplog/change_store/block_meta_encode.rs#L12-L93).
Reader:
[`block_meta_encode.rs::decode_changes_header`](../crates/loro-internal/src/oplog/change_store/block_meta_encode.rs#L90-L242).

### 7.2 `change_meta`

```text
DeltaOfDelta<i64>[N] timestamps
AnyRle<u32>[N]       commit_message_byte_lengths
u8                   concatenated_utf8_messages[sum(lengths)]
EOF
```

A zero message length decodes as `None`; therefore an explicitly present empty
message is not distinguishable from no message in this format.

Writer:
[`block_meta_encode.rs::encode_changes`](../crates/loro-internal/src/oplog/change_store/block_meta_encode.rs#L12-L93).
Reader:
[`block_encode.rs::decode_block`](../crates/loro-internal/src/oplog/change_store/block_encode.rs#L535-L705).

## 8. serde_columnar grammar used below

This section is required to parse `ops`, positions, delete IDs, and state
metadata correctly.

For a `#[columnar(ser, de)]` outer struct with `F` non-skipped fields,
serde_columnar writes a postcard sequence:

```text
pvar(F)
field_0
...
field_F-1
```

A field marked `class = "vec"` is a nested columnar vector. If its row type has
`C` fields, it writes:

```text
pvar(C)
repeat C times:
    bytes column_payload
```

The first varint of `columnar(EncodedOps)` is therefore outer field count `1`,
not operation column count `4`. The nested vector then starts with `4`.

RLE strategy payloads contain only the strategy stream; their row count is
inferred by consuming the payload. A generic (no-strategy) column payload is a
postcard `Vec<T>`, so that payload itself starts with `pvar(row_count)`.

Derive behavior is supplied by serde_columnar_derive 0.3.7, locked transitively
with serde_columnar. Concrete use sites are linked for every structure below;
the locked version is recorded in
[`Cargo.lock`](../Cargo.lock#L2704-L2718).
The exact dependency sources are
`serde_columnar_derive-0.3.7/src/serde/ser.rs` for the outer sequence and
`src/derive/vec.rs` for nested class-vector columns, plus
`serde_columnar-0.3.14/src/column/{serde_impl.rs,mod.rs}` for length-delimited
column payloads.

### 8.1 RLE strategies

`BoolRle` starts with a false run. It writes alternating postcard `usize` run
lengths; the first run may be zero when the first value is true.

`AnyRle<T>` is a sequence of segments. Each segment begins with a postcard
zigzag `isize`:

- positive `k`: repeat the following single postcard `T` value `k` times;
- negative `-k`: read `k` literal postcard `T` values;
- zero: invalid.

`DeltaRle<T>` computes `delta[0] = value[0] - 0` and subsequent differences,
represents deltas as `i128`, and encodes that stream with `AnyRle<i128>`.

`DeltaOfDelta<T>` writes postcard `Option<i64>` for the first value, one byte
describing the number of valid bits in the final bitstream byte, then an
MSB-first delta-of-delta bitstream. The second value's delta-of-delta is
`(value[1] - value[0]) - 0`; it is not automatically zero. The codes are:

| Prefix | Payload bits | Decoded delta-of-delta |
|---|---:|---|
| `0` | 0 | `0` |
| `10` | 7 | payload `- 63` |
| `110` | 9 | payload `- 255` |
| `1110` | 12 | payload `- 2047` |
| `11110` | 21 | payload `- (2^20 - 1)` |
| `11111` | 64 | signed two's-complement `i64` bits |

Exact writers/readers are the strategy types used by the source structs in
sections 7-9. Dependency lock:
[`Cargo.lock`](../Cargo.lock#L2704-L2718).
Algorithm source:
`serde_columnar-0.3.14/src/strategy/rle.rs::{BoolRleEncoder,
AnyRleEncoder,DeltaRleEncoder,DeltaOfDeltaEncoder}` and the corresponding
decoders.

## 9. Operation sections

### 9.1 `ops`

`ops` is `columnar(EncodedOps)`: outer field count `1`, then a four-column
`EncodedOp` vector.

| Column | Rust type | Strategy | Meaning |
|---|---|---|---|
| `container_index` | `u32` | DeltaRle | zero-based index in decoded `cids` |
| `prop` | `i32` | DeltaRle | operation-specific position, key index, or opaque property |
| `value_type` | `u8` | Rle (AnyRle wire) | tag for the next logical payload in `values` |
| `len` | `u32` | Rle (AnyRle wire) | operation atom length |

`value_type` is the only outer value tag. The `values` section is a
concatenation of payload bytes and does not repeat these tags.
The derive attribute spells these two strategies `Rle`; serde_columnar encodes
that strategy with the `AnyRle` segment grammar in section 8.1.

`len` is `Op::atom_len()`, not the payload byte length. There is no per-change
operation count or change index. The decoder walks operation rows in order,
adds their atom lengths, and starts the next change when the accumulated atom
length reaches the next counter boundary reconstructed from `header`. For
canonical bytes, every operation has positive atom length and the sum of op
lengths assigned to each change equals that change's atom length exactly.

Writer/reader struct:
[`block_encode.rs::EncodedOp`, `EncodedOps`](../crates/loro-internal/src/oplog/change_store/block_encode.rs#L414-L435).
Iteration and value dispatch:
[`block_encode.rs::decode_block`](../crates/loro-internal/src/oplog/change_store/block_encode.rs#L589-L705).

### 9.2 `cids`: row-wise postcard, not columnar

`ContainerArena::encode()` serializes the raw `Vec<EncodedContainer>`. Although
`EncodedContainer` also has columnar row traits, no `ColumnarVec` wrapper is
used here. The wire is:

```text
pvar(container_count)
repeat container_count times:
    pvar(4)                    # EncodedContainer row field count
    u8         is_root_bool       # postcard bool: 00 or 01
    u8         container_kind
    pvar(usize peer_index)
    pvar(i32   key_or_counter)    # postcard zigzag
```

For a root container, `peer_index` is zero and `key_or_counter` is an index in
`keys`. For a normal container, `peer_index` indexes the block peer table and
`key_or_counter` is the container's creation counter. Container kind tags are
`0 Map`, `1 List`, `2 Text`, `3 Tree`, `4 MovableList`, and `5 Counter` when the
counter feature is enabled; other `u8` values are preserved as unknown kinds.

The leading `container_count` comes from the ordinary postcard `Vec`. The
per-row `4` comes from `EncodedContainer`'s generated `Serialize` sequence. It
is not the outer `1` that `columnar(ContainerArena)` would have written;
`ContainerArena::encode()` deliberately serializes its inner Vec directly.

Writer/reader:
[`arena.rs::EncodedContainer`, `ContainerArena::encode`, `decode`](../crates/loro-internal/src/encoding/arena.rs#L35-L105),
kind mapping:
[`loro-common::ContainerType::to_u8`, `try_from_u8`](../crates/loro-common/src/lib.rs#L748-L793).

### 9.3 `keys`

Keys are concatenated with no count:

```text
repeat until EOF:
    uleb utf8_byte_len
    u8   utf8[utf8_byte_len]
```

The registry uses first-registration order. It contains map operation keys,
root container names, rich-text mark keys, and keys in nested map values as
needed by this block.

Writer/reader:
[`block_encode.rs::encode_keys`, `decode_keys`](../crates/loro-internal/src/oplog/change_store/block_encode.rs#L280-L305).

### 9.4 `positions`

Tree fractional-index byte strings are sorted in lexicographic raw-byte order,
deduplicated, and registered before operation encoding. `FractionalIndex` gets
that order from the derived `Ord` of its `Vec<u8>`. A block with no positions
writes a zero-length `positions` section.
Otherwise it writes `columnar(PositionArena)`:

```text
pvar(1)                     # PositionArena outer field count
pvar(2)                     # PositionDelta nested column count
bytes Rle<usize> common_prefix_length
bytes Generic<Cow<[u8]>> rest
```

The generic `rest` payload is itself `pvar(row_count)` followed by
`pvar(byte_len) + bytes` for each row. Row 0 must have common prefix zero. For
row `i > 0`, reconstruct `position[i]` by copying the prefix from
`position[i-1]` and appending `rest[i]`.

Writer/reader:
[`arena.rs::PositionDelta`, `PositionArena`](../crates/loro-internal/src/encoding/arena.rs#L158-L253),
ordering before registration:
[`block_encode.rs::encode_block`](../crates/loro-internal/src/oplog/change_store/block_encode.rs#L151-L176).
Position ordering:
[`FractionalIndex`](../crates/fractional_index/src/lib.rs#L15-L18).

### 9.5 `delete_start_ids`

If there is no list/text delete operation, this section is zero bytes.
Otherwise it is `columnar(EncodedDeleteStartIds)`:

```text
pvar(1)  # outer field count
pvar(3)  # nested column count
bytes DeltaRle<usize> peer_index
bytes DeltaRle<i32>   counter
bytes DeltaRle<isize> signed_len
```

There is one row for each known Text, List, or MovableList `DeleteSeq`
operation, in operation order. The row supplies the target ID
`(peer_table[peer_index], counter)` and signed deletion length; the operation's
`prop` supplies the positional start. An Unknown-container operation may carry
the same value tag opaquely; it neither writes nor consumes a delete-start row.

Writer:
[`outdated_encode_reordered.rs::encode_op`](../crates/loro-internal/src/encoding/outdated_encode_reordered.rs#L132-L156),
row schema:
[`EncodedDeleteStartId`](../crates/loro-internal/src/encoding/outdated_encode_reordered.rs#L480-L491),
wrapper and reader:
[`block_encode.rs::EncodedDeleteStartIds`, `decode_block`](../crates/loro-internal/src/oplog/change_store/block_encode.rs#L437-L445),
[`block_encode.rs`](../crates/loro-internal/src/oplog/change_store/block_encode.rs#L589-L603).

### 9.6 Operation mapping emitted by the current writer

| Container / operation | `prop` | `value_type` | Payload or side table |
|---|---|---:|---|
| Text insert | Unicode-scalar position | 5 `Str` | `uleb(utf8_len) + utf8` |
| Text delete | signed positional start | 9 `DeleteSeq` | no value bytes; consume one delete-start row |
| Text style start | style start position | 12 `MarkStart` | mark payload in section 10 |
| Text style end | `0` | 0 `Null` | no value bytes |
| List insert | list position | 11 `LoroValue` | nested value must be a list of inserted values |
| List delete | list position | 9 `DeleteSeq` | no value bytes; consume one delete-start row |
| Map set | key index | 11 `LoroValue` | nested value |
| Map delete | key index | 8 `DeleteOnce` | no value bytes |
| MovableList insert/delete | same as List | 11 / 9 | same as List |
| MovableList move | destination position | 14 `ListMove` | source position, peer-table index and Lamport identifying the element |
| MovableList set | `0` | 15 `ListSet` | peer-table index and Lamport identifying the element, then nested value |
| Tree create/move/delete | `0` | 16 `RawTreeMove` | raw tree payload; delete names the reserved deleted root as parent |
| Counter delta, counter feature | `0` | 3 `I64` or 4 `F64` | exact tag-selection rule below |
| Unknown container operation | preserved opaque `i32` | preserved `OwnedValue` kind | kind-specific payload |

The canonical `len` column is semantic atom count, not payload size:

- Text insert: Unicode-scalar count;
- List or MovableList insert: number of top-level inserted elements;
- Text, List, or MovableList `DeleteSeq`: `signed_len.unsigned_abs()` from its
  delete-start row; and
- every other operation row in the table, including style anchors, move/set,
  Map, Tree, Counter, and Unknown operations: `1`.

In particular, MarkStart's mark length and a string's UTF-8 byte length are not
the operation `len`. Length definitions:
[`InnerListOp::content_len`](../crates/loro-internal/src/container/list/list_op.rs#L591-L603),
[`InnerContent::content_len`](../crates/loro-internal/src/op/content.rs#L220-L229),
[`DeleteSpan::content_len`](../crates/loro-internal/src/container/list/list_op.rs#L124-L130).

For a Counter delta `d`, the writer computes `a = abs(d)` and chooses tag 3
I64 exactly when `a.fract() < f64::EPSILON && (a as i64) < (2 << 26)`; it then
writes `d as i64`. Otherwise it chooses tag 4 F64. This is a one-sided
fractional-part test, not a symmetric distance-to-nearest-integer test.

The decoder retains support for tag 13 `TreeMove`, but the mode-4 writer uses
tag 16 `RawTreeMove`. In the current change-block decoder, tag 13 cannot be
resolved for a known Tree container because its decode arena intentionally
does not implement the older tree arena mapping; it can still be preserved as
an owned value for an unknown container. New compatible writers must use tag
16 for Tree operations.

Property writer and value selection:
[`outdated_encode_reordered.rs::get_op_prop`, `encode_op`](../crates/loro-internal/src/encoding/outdated_encode_reordered.rs#L101-L215).
Operation reader:
[`outdated_encode_reordered.rs::decode_op`](../crates/loro-internal/src/encoding/outdated_encode_reordered.rs#L215-L475).
Current tree register:
[`block_encode.rs::Registers::encode_tree_op`](../crates/loro-internal/src/oplog/change_store/block_encode.rs#L329-L366).

## 10. `values` payload encoding

The values section is read once in operation order. Tags live in the parallel
`ops.value_type` column. Zero-payload kinds do not advance the values cursor.

| Tag | Kind | Payload in `values` |
|---:|---|---|
| 0 | Null | none |
| 1 | True | none |
| 2 | False | none |
| 3 | I64 | `sleb(i64)` |
| 4 | F64 | 8-byte IEEE-754 **big-endian** |
| 5 | Str | `uleb(utf8_len) + utf8` |
| 6 | Binary | `uleb(byte_len) + bytes` |
| 7 | ContainerIdx | `uleb(index)`; opaque `Value::ContainerIdx`, not a `cids` reference and not a container-kind byte |
| 8 | DeleteOnce | none |
| 9 | DeleteSeq | none; known Text/List/MovableList deletes use `delete_start_ids`, while an Unknown-container opaque value uses no side row |
| 10 | DeltaInt | `sleb(i32)` |
| 11 | LoroValue | nested tag and payload below |
| 12 | MarkStart | mark payload below |
| 13 | TreeMove | older tree payload below; not emitted for current known Tree ops |
| 14 | ListMove | three `uleb` values |
| 15 | ListSet | two `uleb` values, then nested LoroValue |
| 16 | RawTreeMove | current tree payload below |
| `0x80 | k` | opaque future kind | canonical writer form for low-seven-bit `k` in `17..=127`; payload is `uleb(len) + bytes` |

The decoder first masks off bit 7. Therefore `0x80 | k` for `k <= 16` decodes
as the known kind rather than as opaque future data. For `k` in `17..=127`,
the current reader also accepts the bare byte `k` and aliases it to the same
opaque future kind as `0x80 | k`. Thus canonical future-tag bytes are
`91..ff`; bare `11..7f` is reader compatibility only and must not be emitted by
a new writer.

Tag 7 is a legacy/generic value form. The current known Text, Map, List,
MovableList, Tree, and Counter operation decoders do not accept it. It can be
retained only as generic data for an Unknown container operation; it is never
resolved through the block's `cids` arena.

Tags and dispatch:
[`value.rs::ValueKind::to_u8`, `from_u8`](../crates/loro-internal/src/encoding/value.rs#L39-L161),
[`Value::decode`, `encode`](../crates/loro-internal/src/encoding/value.rs#L342-L459).
Primitive reader/writer:
[`value.rs::ValueReader`](../crates/loro-internal/src/encoding/value.rs#L860-L935),
[`ValueWriter`](../crates/loro-internal/src/encoding/value.rs#L1048-L1097).

### 10.1 Nested LoroValue

A nested LoroValue begins with one raw `u8` tag:

| Tag | Value | Following payload |
|---:|---|---|
| 0 | Null | none |
| 1 | true | none |
| 2 | false | none |
| 3 | I64 | `sleb(i64)` |
| 4 | F64 | 8-byte big-endian |
| 5 | String | `uleb(utf8_len) + utf8` |
| 6 | Binary | `uleb(len) + bytes` |
| 7 | List | `uleb(item_count)` then recursively tagged values |
| 8 | Map | `uleb(entry_count)` then `uleb(key_index)` plus recursively tagged value for each entry |
| 9 | Container | one raw container-kind `u8`; reconstruct a normal ContainerID at the current operation ID |

Nested map key indices refer to the block `keys` registry. Map iteration order
is not a semantic part of the value. Each List or Map collection length must be
at most `1 << 28` for the current reader.

The container payload deliberately omits a ContainerID. Whenever the outermost
kind of a decoded LoroValue is List, direct list element `i` is decoded with
logical ID `op_id.inc(i)`. This rule is not limited to List-insert operations:
it also applies if a Map set, ListSet, mark value, or another LoroValue-bearing
operation has a top-level List. A List or Map nested below one of those direct
elements does not start another increment sequence; its descendants inherit
that element ID. Other top-level value shapes use the operation ID. The one
kind byte plus this contextual ID reconstructs the normal ContainerID.

Writer/reader:
[`value.rs::LoroValueKind`](../crates/loro-internal/src/encoding/value.rs#L62-L95),
[`ValueReader::read_value_type_and_content`](../crates/loro-internal/src/encoding/value.rs#L608-L669),
[`ValueWriter::write_value_type_and_content`](../crates/loro-internal/src/encoding/value.rs#L1000-L1046).
Collection limit and contextual-ID reader:
[`MAX_COLLECTION_SIZE`](../crates/loro-internal/src/encoding/outdated_encode_reordered.rs#L28-L31),
[`ValueReader::read_value_content`, recursive reader](../crates/loro-internal/src/encoding/value.rs#L608-L859).

### 10.2 MarkStart

```text
u8   info
uleb mark_len_in_unicode_scalars
uleb key_index
      nested_loro_value
```

`info` uses bit `0x80` for alive, `0x02` for expand-before, and `0x04` for
expand-after. Other bits are not assigned by the current writer.

Writer/reader:
[`value.rs::write_mark`, `read_mark`](../crates/loro-internal/src/encoding/value.rs#L936-L955),
[`value.rs`](../crates/loro-internal/src/encoding/value.rs#L1099-L1107).
Flag definition:
[`richtext.rs::TextStyleInfoFlag`](../crates/loro-internal/src/container/richtext.rs#L143-L160),
[`TextStyleInfoFlag::new`](../crates/loro-internal/src/container/richtext.rs#L228-L284).

### 10.3 Current RawTreeMove (tag 16)

```text
uleb subject_peer_index
uleb subject_counter_as_nonnegative_usize
uleb position_index
u8   is_parent_null              # zero=false, nonzero=true when decoding
if is_parent_null == 0:
    uleb parent_peer_index
    uleb parent_counter_as_nonnegative_usize
```

Create is distinguished from move when the subject ID equals the operation ID.
Delete is encoded with `position_index = 0`, `is_parent_null = 0`, and the
reserved deleted-root parent
`(peer = 0xffffffffffffffff, counter = 0x7fffffff)`. The writer registers that
maximum peer in the block peer table; `parent_peer_index` names that entry and
the parent counter's ULEB128 bytes are `ff ff ff ff 07`. The delete decoder
does not consult the position index. The positions arena may therefore be
empty: this zero is an ignored placeholder and need not be an in-range arena
index.

Writer/reader:
[`value.rs::write_raw_tree_move`, `read_raw_tree_move`](../crates/loro-internal/src/encoding/value.rs#L969-L991),
[`value.rs`](../crates/loro-internal/src/encoding/value.rs#L1125-L1140).
Semantic reconstruction:
[`outdated_encode_reordered.rs::decode_op`](../crates/loro-internal/src/encoding/outdated_encode_reordered.rs#L331-L392).
Canonical delete register and constants:
[`Registers::encode_tree_op`](../crates/loro-internal/src/oplog/change_store/block_encode.rs#L329-L366),
[`DELETED_TREE_ROOT`](../crates/loro-common/src/lib.rs#L1152-L1159).

### 10.4 Older TreeMove (tag 13)

```text
uleb target_tree_id_index
u8   is_parent_null
uleb position_index
if is_parent_null == 0:
    uleb parent_tree_id_index
```

This layout remains in the value decoder for compatibility but is not the
layout a current writer should choose for a known Tree operation.

Writer/reader helpers:
[`value.rs::write_tree_move`, `read_tree_move`](../crates/loro-internal/src/encoding/value.rs#L956-L968),
[`value.rs`](../crates/loro-internal/src/encoding/value.rs#L1109-L1123).

### 10.5 ListMove and ListSet

```text
ListMove:
    uleb source_position
    uleb element_peer_table_index
    uleb element_lamport

ListSet:
    uleb element_peer_table_index
    uleb element_lamport
         nested_loro_value
```

For `ListMove`, `prop` is the destination position and `source_position` is the
old positional index. For both forms, `(peers[element_peer_table_index],
element_lamport)` is the stable movable-list element ID; the second field is
not another position.

Writer/reader:
[`value.rs::Value::decode`, `encode`](../crates/loro-internal/src/encoding/value.rs#L342-L459).

### 10.6 Canonical change-block invariants

The current writer always produces all of the following:

- `n_changes >= 1`; all changes share `peers[0]` and are counter-contiguous;
- counters fit the nonnegative CRDT `i32` domain, Lamports fit `u32`, and all
  reconstructed span arithmetic is exact and non-overflowing;
- `header` and `change_meta` describe exactly `n_changes` changes;
- every peer, key, and container index is in range, as is every position index
  that its operation dereferences; Tree delete's ignored zero placeholder is
  exempt and may accompany an empty positions arena;
- operation atom lengths partition the reconstructed change counter ranges
  exactly, without crossing a boundary;
- each known Text/List/MovableList `DeleteSeq` consumes exactly one
  delete-start row and no other known operation consumes one; an opaque
  Unknown-container `DeleteSeq` consumes none;
- every operation consumes exactly its kind-specific value payload, leaving no
  value bytes; and
- commit-message lengths consume the concatenated UTF-8 message bytes exactly.

Malformed varints, truncated byte fields, invalid UTF-8, invalid strategy
streams, or arithmetic/index failures are errors. A zero-length outer mode-4
block cannot contain an `EncodedBlock` and is invalid.

Some current Rust read paths are deliberately or accidentally more tolerant
than those canonical invariants. Postcard block decoding does not expose its
remainder, the header remainder is only a debug assertion, and `decode_block`
does not perform one final exhaustion check for commit-message bytes, value
bytes, or delete rows. Its operation loop also does not compare encoded `len`
with the decoded content's atom length or check the final counter and change
index. It advances at most one change on `counter >= next_counter`. As a
result, this layer can return successfully for inputs with missing operations,
zero operation lengths, or lengths that cross a change boundary, provided the
other reads remain in bounds. These are not reserved extension areas and not a
compatibility promise. Writers must emit the canonical exact-consumption and
exact-partition form above; independent validators should reject these inputs.

Header validation:
[`decode_changes_header`](../crates/loro-internal/src/oplog/change_store/block_meta_encode.rs#L90-L242).
Current operation assembly and its exhaustion boundary:
[`decode_block`](../crates/loro-internal/src/oplog/change_store/block_encode.rs#L535-L705).

## 11. State SSTable schema

State section entries are:

| Key | Value |
|---|---|
| `ContainerID::to_bytes()` | `ContainerWrapper::encode()` |
| ASCII `fr` | only in shallow-root state; `Frontiers::encode(S)` |

`state_bytes` overlays do not carry `fr`; import merges the root and overlay KV
layers, removes the root `fr`, and only then scans and registers live container
state. Container keys and values, including every container-specific state
codec, are specified in
[encoding-container-states.md](./encoding-container-states.md).

Schema and root key:
[`container_store.rs::FRONTIERS_KEY`](../crates/loro-internal/src/state/container_store.rs#L48-L49),
[`inner_store.rs::flush`, `decode`, `decode_twice`](../crates/loro-internal/src/state/container_store/inner_store.rs#L221-L317).

## 12. Endianness summary

There is no single endianness rule for the whole format. Use the rule at each
field:

| Context | Encoding |
|---|---|
| document snapshot section lengths | fixed `u32le` |
| document mode | fixed `u16be` |
| document/SSTable checksums | fixed `u32le` |
| SSTable integers | fixed little-endian |
| ChangeStore 12-byte ID keys | peer `u64be`, counter `i32be` |
| state ContainerID normal keys | peer `u64le`, counter `i32le` |
| raw peer tables in change/state codecs | fixed `u64le` |
| postcard unsigned/signed integers | unsigned LEB128 / zigzag unsigned LEB128 |
| custom operation signed integers | signed LEB128 |
| custom operation F64 | fixed big-endian |
| postcard F64 and CounterState F64 | fixed little-endian |

Sources are the field-specific writer/reader links above. The two distinct
ContainerID encodings and postcard state values are detailed in
[encoding-container-states.md](./encoding-container-states.md).

## 13. Decoder conformance checklist

A compatible decoder should, at minimum:

1. verify lowercase document magic, mode, and the checksum over `blob[20..]`;
2. distinguish `E` from a zero-byte state KV store;
3. require exact EOF after the three snapshot sections;
4. treat an empty SSTable section as an empty KV store before looking for
   uppercase `LORO`;
5. verify SSTable metadata checksum and bounds before lazy block reads;
6. parse updates until exact EOF and reject overflowing or truncated block
   lengths;
7. account for both outer struct counts and nested column counts in every
   serde_columnar structure;
8. read value tags from `ops.value_type`, not from `values`;
9. keep `sleb` separate from postcard zigzag integers;
10. treat VersionVector maps as unordered and counters as exclusive ends;
11. overlay shallow end state after root state, or replay when the section is
    `E`; and
12. preserve or reject unknown kinds explicitly rather than interpreting their
    payload as a known container.

Primary validation paths in this repository:

- [`fast_snapshot.rs` snapshot structural reader](../crates/loro-internal/src/encoding/fast_snapshot.rs#L47-L126),
- [`fast_snapshot.rs` updates structural reader](../crates/loro-internal/src/encoding/fast_snapshot.rs#L360-L388),
- [`sstable.rs` structural validation](../crates/kv-store/src/sstable.rs#L369-L518),
- [`block_encode.rs::decode_block`](../crates/loro-internal/src/oplog/change_store/block_encode.rs#L535-L705), and
- [`import_atomicity.rs`](../crates/loro-internal/src/tests/import_atomicity.rs).
