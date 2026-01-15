# Moonbit Loro Codec – SPEC NOTES

This file records implementation-critical notes for the MoonBit codec under `moon/`,
as requested by `moon/specs/01-context-checklist.md`.

It is intentionally **not** a full spec: the source of truth is `docs/encoding.md`
and the Rust implementation referenced below.

## Endianness (must match Rust)

- **Document mode** (`u16`) is **big-endian** (bytes `[20..22]` in the document header).
- **Document checksum** (`xxHash32`) is stored as **u32 little-endian** in bytes `[16..20]`,
  and the checksum covers **bytes `[20..]`** (mode + body), not just body.
- **ChangeBlock key** is 12 bytes: `peer(u64 BE) + counter(i32 BE)`.
- **ID.to_bytes** (peer+counter) uses **big-endian** for both peer and counter.
- **Custom ValueEncoding**:
  - `F64` is **big-endian** IEEE754.
  - `I64`/`DeltaInt` use **SLEB128** (two’s complement sign extension), not zigzag.
- **postcard** uses **unsigned varint + zigzag** (different from SLEB128).

## Integer encodings used

- **ULEB128/SLEB128**: used in document bodies (FastUpdates block lengths), `keys` arena,
  and the custom value encoding.
- **postcard varint + zigzag**: used by postcard itself and by serde_columnar.

## ContainerType mappings (two tables)

- **Binary ContainerID / ContainerWrapper kind byte** (`ContainerType::to_bytes` mapping):
  `Map=0, List=1, Text=2, Tree=3, MovableList=4, Counter=5`.
- **Historical mapping** (only for postcard `Option<ContainerID>` in wrapper.parent):
  `Text=0, Map=1, List=2, MovableList=3, Tree=4, Counter=5`.

See Rust: `crates/loro-internal/src/state/container_store/container_wrapper.rs`.

## Unicode (RichText)

- Text positions for snapshot decoding and JsonSchema use **Unicode scalar count**
  (not UTF-16 code units).
- Moon implementation uses `count_utf8_codepoints(...)` when converting between
  string lengths and the on-wire representation.

## serde_columnar i128

- DeltaRle/DeltaOfDelta conceptually operate on i128 deltas.
- Moon implementation uses `BigInt` as the internal accumulator for i128-like behavior
  (see `moon/loro_codec/serde_columnar_delta_rle.mbt`).

## LZ4 Frame (SSTable compression)

- SSTable blocks may be compressed using **LZ4 Frame**, as in Rust (`lz4_flex::frame`).
- Moon supports:
  - decoding frames (`lz4_decompress_frame`)
  - encoding frames (`lz4_compress_frame`) using block-independence and BD=64KB
  - per-block compression fallback: if LZ4 frame output is larger than raw, encode as `CompressionType::None`

## Forward/unknown handling

- Custom ValueEncoding keeps unknown tags as opaque bytes (`Value::Future(tag, data)`),
  enabling conservative round-tripping at the value layer.
- JsonSchema import still rejects:
  - `UnknownOp` (forward-compat op content) for non-Counter containers
  - root container values (`🦜:cid:root-...`) because binary container values
    reconstruct IDs from `op_id + container_type` and cannot represent roots.

## Rust “truth” pointers (for debugging)

- Document header/body: `crates/loro-internal/src/encoding.rs`, `.../encoding/fast_snapshot.rs`
- SSTable: `crates/kv-store/src/sstable.rs`, `crates/kv-store/src/block.rs`, `crates/kv-store/src/compress.rs`
- ChangeBlock: `crates/loro-internal/src/oplog/change_store/block_encode.rs`,
  `crates/loro-internal/src/oplog/change_store/block_meta_encode.rs`,
  `crates/loro-internal/src/encoding/outdated_encode_reordered.rs`
- Value encoding: `crates/loro-internal/src/encoding/value.rs`
- IDs / ContainerIDs: `crates/loro-common/src/lib.rs`

