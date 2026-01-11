# Loro Binary Encoding Format

## Introduction

This document describes the binary encoding format used by Loro for exporting and importing documents. The format is designed for efficient storage and fast synchronization of CRDT operations across peers.

Understanding this specification will enable developers to implement Loro-compatible encoders and decoders in other programming languages.

## Table of Contents

- [Export Modes](#export-modes)
- [Overall Binary Structure](#overall-binary-structure)
- [FastSnapshot Format](#fastsnapshot-format)
- [FastUpdates Format](#fastupdates-format)
- [KV Store (SSTable) Format](#kv-store-sstable-format)
- [OpLog Encoding](#oplog-encoding)
- [State Encoding](#state-encoding)
- [Change Block Encoding](#change-block-encoding)
- [Value Encoding](#value-encoding)
- [Compression Techniques](#compression-techniques)

---

## Export Modes

Loro supports multiple export modes to meet different synchronization requirements:

| Mode | EncodeMode Value | Description |
|------|------------------|-------------|
| `Snapshot` | `FastSnapshot (3)` | Full history + current state |
| `Updates` | `FastUpdates (4)` | History since a version vector |
| `UpdatesInRange` | `FastUpdates (4)` | History in specified ID spans |
| `ShallowSnapshot` | `FastSnapshot (3)` | Partial history since target frontiers (like Git shallow clone) |
| `StateOnly` | `FastSnapshot (3)` | State-only snapshot with minimal history (depth=1) |
| `SnapshotAt` | `FastSnapshot (3)` | Full history till target version + state at that version |

**Source**: `crates/loro-internal/src/encoding.rs:52-70`

---

## Overall Binary Structure

All Loro export formats share a common 22-byte header:

```
┌─────────────────────────────────────────────────────────────────┐
│                      Loro Binary Format                          │
├───────────────┬─────────────────────────────────────────────────┤
│ Offset        │ Content                                         │
├───────────────┼─────────────────────────────────────────────────┤
│ 0..4          │ Magic Bytes: "loro" (0x6C6F726F)                │
├───────────────┼─────────────────────────────────────────────────┤
│ 4..20         │ Checksum (16 bytes)                             │
│               │   - [0..12]: Reserved (zeros)                   │
│               │   - [12..16]: xxHash32 of body (little-endian)  │
│               │     Seed: 0x4F524F4C ("LORO" in LE)             │
├───────────────┼─────────────────────────────────────────────────┤
│ 20..22        │ Encode Mode (big-endian u16)                    │
│               │   - 1: OutdatedRle (unsupported)                │
│               │   - 2: OutdatedSnapshot (unsupported)           │
│               │   - 3: FastSnapshot                             │
│               │   - 4: FastUpdates                              │
├───────────────┼─────────────────────────────────────────────────┤
│ 22..          │ Body (format depends on mode)                   │
└───────────────┴─────────────────────────────────────────────────┘
```

### Checksum Calculation

For `FastSnapshot` and `FastUpdates` modes:

```
checksum = xxHash32(body_bytes, seed=0x4F524F4C)
```

The checksum is stored in bytes [16..20] of the header as a little-endian u32.

**Source**: `crates/loro-internal/src/encoding.rs:275-295, 397-416`

---

## FastSnapshot Format

The FastSnapshot body contains three sections:

```
┌─────────────────────────────────────────────────────────────────┐
│                     FastSnapshot Body                            │
├───────────────┬─────────────────────────────────────────────────┤
│ Bytes         │ Content                                         │
├───────────────┼─────────────────────────────────────────────────┤
│ 4             │ oplog_bytes length (u32 little-endian)          │
├───────────────┼─────────────────────────────────────────────────┤
│ variable      │ oplog_bytes (KV Store encoded)                  │
│               │ Contains all Change operations history          │
├───────────────┼─────────────────────────────────────────────────┤
│ 4             │ state_bytes length (u32 little-endian)          │
├───────────────┼─────────────────────────────────────────────────┤
│ variable      │ state_bytes (KV Store encoded)                  │
│               │ Contains current document state                 │
│               │ Special: Single byte "E" (0x45) if empty        │
├───────────────┼─────────────────────────────────────────────────┤
│ 4             │ shallow_root_state_bytes length (u32 LE)        │
├───────────────┼─────────────────────────────────────────────────┤
│ variable      │ shallow_root_state_bytes (KV Store encoded)     │
│               │ Only present for shallow snapshots              │
│               │ Empty (length=0) for normal snapshots           │
└───────────────┴─────────────────────────────────────────────────┘
```

**Source**: `crates/loro-internal/src/encoding/fast_snapshot.rs:1-46`

### Encoding Pseudocode

```rust
fn encode_snapshot(snapshot: Snapshot) -> Vec<u8> {
    let mut buf = Vec::new();

    // OpLog bytes
    buf.write_u32_le(snapshot.oplog_bytes.len() as u32);
    buf.write_all(&snapshot.oplog_bytes);

    // State bytes (or "E" if empty)
    let state = snapshot.state_bytes.unwrap_or(b"E");
    buf.write_u32_le(state.len() as u32);
    buf.write_all(&state);

    // Shallow root state bytes
    buf.write_u32_le(snapshot.shallow_root_state_bytes.len() as u32);
    buf.write_all(&snapshot.shallow_root_state_bytes);

    buf
}
```

---

## FastUpdates Format

The FastUpdates body contains a sequence of LEB128-length-prefixed change blocks:

```
┌─────────────────────────────────────────────────────────────────┐
│                     FastUpdates Body                             │
├───────────────┬─────────────────────────────────────────────────┤
│ Bytes         │ Content                                         │
├───────────────┼─────────────────────────────────────────────────┤
│ LEB128        │ block_1 length                                  │
├───────────────┼─────────────────────────────────────────────────┤
│ variable      │ block_1 bytes (Change Block format)             │
├───────────────┼─────────────────────────────────────────────────┤
│ LEB128        │ block_2 length                                  │
├───────────────┼─────────────────────────────────────────────────┤
│ variable      │ block_2 bytes (Change Block format)             │
├───────────────┼─────────────────────────────────────────────────┤
│ ...           │ (repeats until end of body)                     │
└───────────────┴─────────────────────────────────────────────────┘
```

**Source**: `crates/loro-internal/src/encoding/fast_snapshot.rs:257-288`

---

## KV Store (SSTable) Format

Both `oplog_bytes` and `state_bytes` use a KV Store format based on SSTable (Sorted String Table).

### SSTable Overall Structure

```
┌─────────────────────────────────────────────────────────────────┐
│                        SSTable Format                            │
├───────────────┬─────────────────────────────────────────────────┤
│ Bytes         │ Content                                         │
├───────────────┼─────────────────────────────────────────────────┤
│ 4             │ Magic Number: "LORO" (0x4C4F524F little-endian) │
├───────────────┼─────────────────────────────────────────────────┤
│ 1             │ Schema Version (currently 0)                    │
├───────────────┼─────────────────────────────────────────────────┤
│ variable      │ Block Chunk 1 (possibly LZ4 compressed)         │
├───────────────┼─────────────────────────────────────────────────┤
│ variable      │ Block Chunk 2                                   │
├───────────────┼─────────────────────────────────────────────────┤
│ ...           │ Block Chunk N                                   │
├───────────────┼─────────────────────────────────────────────────┤
│ variable      │ Block Meta (metadata for all blocks)            │
├───────────────┼─────────────────────────────────────────────────┤
│ 4             │ meta_offset (u32 little-endian)                 │
│               │ Offset of Block Meta from start of SSTable      │
└───────────────┴─────────────────────────────────────────────────┘
```

**Source**: `crates/kv-store/src/sstable.rs:254-295`

### Block Meta Format

```
┌─────────────────────────────────────────────────────────────────┐
│                       Block Meta Section                         │
├───────────────┬─────────────────────────────────────────────────┤
│ Bytes         │ Content                                         │
├───────────────┼─────────────────────────────────────────────────┤
│ 4             │ Number of blocks (u32 little-endian)            │
├───────────────┼─────────────────────────────────────────────────┤
│               │ For each block:                                 │
│ 4             │   block_offset (u32 little-endian)              │
│ 2             │   first_key_len (u16 little-endian)             │
│ variable      │   first_key (bytes)                             │
│ 1             │   flags: is_large(1bit) | compression_type(7bit)│
│ 2             │   last_key_len (u16 LE) - only if !is_large     │
│ variable      │   last_key (bytes) - only if !is_large          │
├───────────────┼─────────────────────────────────────────────────┤
│ 4             │ Checksum (xxHash32, little-endian)              │
│               │ Covers all block meta entries (excluding count) │
└───────────────┴─────────────────────────────────────────────────┘
```

### Compression Types

| Value | Type |
|-------|------|
| 0 | None |
| 1 | LZ4 |

**Source**: `crates/kv-store/src/sstable.rs:23-141`

### Block Chunk Format

- [ ] **TODO: Document Block Chunk internal encoding format**
  - Key-value pair encoding within blocks
  - Key prefix compression
  - Block builder logic

**Source**: `crates/kv-store/src/block.rs`

---

## OpLog Encoding

The OpLog is stored as a KV Store with the following key-value schema:

### OpLog KV Schema

| Key | Value |
|-----|-------|
| `b"vv"` (2 bytes) | VersionVector encoding |
| `b"fr"` (2 bytes) | Frontiers encoding |
| `b"sv"` (2 bytes) | Shallow start VersionVector |
| `b"sf"` (2 bytes) | Shallow start Frontiers |
| 12 bytes: PeerID (8) + Counter (4) | Encoded Change Block |

**Source**: `crates/loro-internal/src/oplog/change_store.rs:47-59, 110-113`

### Block Key Format

For change blocks, the key is 12 bytes:

```
┌─────────────────────────────────────────────────────────────────┐
│                    Change Block Key (12 bytes)                   │
├───────────────┬─────────────────────────────────────────────────┤
│ Bytes         │ Content                                         │
├───────────────┼─────────────────────────────────────────────────┤
│ 0..8          │ PeerID (u64 little-endian)                      │
├───────────────┼─────────────────────────────────────────────────┤
│ 8..12         │ Counter (u32 little-endian) - start of block    │
└───────────────┴─────────────────────────────────────────────────┘
```

### VersionVector Encoding

- [ ] **TODO: Document VersionVector binary encoding format**

**Source**: `crates/loro-common/src/span.rs`

### Frontiers Encoding

- [ ] **TODO: Document Frontiers binary encoding format**

**Source**: `crates/loro-internal/src/version.rs`

---

## State Encoding

The document state is stored as a KV Store where each container's state is a separate entry.

### State KV Schema

| Key | Value |
|-----|-------|
| `ContainerID.to_bytes()` | ContainerWrapper encoded bytes |
| `b"FRONTIERS_KEY"` | Frontiers encoding (for shallow snapshots) |

### ContainerID Encoding

- [ ] **TODO: Document ContainerID.to_bytes() format**
  - Root container encoding
  - Normal container encoding (with ID)

**Source**: `crates/loro-common/src/container_id.rs`

### ContainerWrapper Encoding

- [ ] **TODO: Document ContainerWrapper binary format**
  - Parent information
  - Container state data
  - Per-container-type encoding (Map, List, Text, Tree, MovableList, Counter, Unknown)

**Source**: `crates/loro-internal/src/state/container_store/container_wrapper.rs`

---

## Change Block Encoding

Each Change Block contains multiple consecutive Changes from the same peer. Blocks are approximately 4KB after compression.

**Source**: `crates/loro-internal/src/oplog/change_store/block_encode.rs:1-65`

### Block Structure Overview

```
┌──────────────────────────────────────────────────────────────────┐
│                       Change Block Layout                         │
├──────────────────────────────────────────────────────────────────┤
│ HEADER SECTION (encoded with postcard)                           │
│ ┌──────────────────────────────────────────────────────────────┐ │
│ │ counter_start: u32     - Starting counter value              │ │
│ │ counter_len: u32       - Counter range length                │ │
│ │ lamport_start: u32     - Starting lamport timestamp          │ │
│ │ lamport_len: u32       - Lamport range length                │ │
│ │ n_changes: u32         - Number of changes in block          │ │
│ │ header: bytes          - Change header metadata              │ │
│ │ change_meta: bytes     - Timestamps and commit messages      │ │
│ │ cids: bytes            - Container IDs                       │ │
│ │ keys: bytes            - Key strings                         │ │
│ │ positions: bytes       - Fractional index positions (Tree)   │ │
│ │ ops: bytes             - Encoded operations                  │ │
│ │ delete_start_ids: bytes- Delete operation start IDs          │ │
│ │ values: bytes          - Value data                          │ │
│ └──────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────┘
```

### Header Section Detail

The `header` field contains:

```
┌──────────────────────────────────────────────────────────────────┐
│                      Header Field Layout                          │
├──────────────────────────────────────────────────────────────────┤
│ LEB128: counter_start                                            │
│ LEB128: counter_len                                              │
│ LEB128: lamport_start                                            │
│ LEB128: lamport_len                                              │
│ LEB128: n_changes (N)                                            │
│ LEB128: peer_num                                                 │
│ [u64 × peer_num]: PeerIDs (8 bytes each, little-endian)          │
│ [LEB128 × N]: Change atom lengths                                │
│ BoolRle: dep_on_self flags (N entries)                           │
│ DeltaRle: dependency lengths (N entries)                         │
│ [encoded deps]: Dependency IDs                                   │
│ [LEB128 × N]: Delta lamports                                     │
└──────────────────────────────────────────────────────────────────┘
```

### Change Meta Section

```
┌──────────────────────────────────────────────────────────────────┐
│                    Change Meta Field Layout                       │
├──────────────────────────────────────────────────────────────────┤
│ DeltaOfDelta encoded: Timestamps (N entries, i64)                │
│ AnyRle encoded: Commit message lengths (N entries, u32)          │
│ Raw bytes: Commit messages (concatenated UTF-8 strings)          │
└──────────────────────────────────────────────────────────────────┘
```

**Source**: `crates/loro-internal/src/oplog/change_store/block_meta_encode.rs`

### Operations Section

The operations are encoded using `serde_columnar` for columnar storage:

```
┌──────────────────────────────────────────────────────────────────┐
│                    Operations Encoding (ops field)                │
├──────────────────────────────────────────────────────────────────┤
│ serde_columnar encoded EncodedOps:                               │
│   - container_index: u32 (DeltaRle strategy)                     │
│   - prop: i32 (DeltaRle strategy)                                │
│   - value_type: u8 (Rle strategy)                                │
│   - len: u32 (Rle strategy)                                      │
└──────────────────────────────────────────────────────────────────┘
```

### Container IDs Section (cids)

- [ ] **TODO: Document ContainerArena encoding format**

**Source**: `crates/loro-internal/src/encoding/arena.rs`

### Key Strings Section (keys)

```
┌──────────────────────────────────────────────────────────────────┐
│                    Key Strings Encoding                           │
├──────────────────────────────────────────────────────────────────┤
│ For each key:                                                    │
│   LEB128: key length                                             │
│   bytes: UTF-8 encoded key string                                │
└──────────────────────────────────────────────────────────────────┘
```

### Positions Section

- [ ] **TODO: Document PositionArena encoding format (for Tree operations)**

**Source**: `crates/loro-internal/src/encoding/arena.rs`

### Delete Start IDs Section

```
┌──────────────────────────────────────────────────────────────────┐
│              Delete Start IDs (serde_columnar encoded)            │
├──────────────────────────────────────────────────────────────────┤
│ EncodedDeleteStartId:                                            │
│   - peer_idx: u32 (DeltaRle)                                     │
│   - counter: i32 (DeltaRle)                                      │
│   - len: i32 (Rle)                                               │
└──────────────────────────────────────────────────────────────────┘
```

**Source**: `crates/loro-internal/src/encoding/outdated_encode_reordered.rs`

### Values Section

- [ ] **TODO: Document ValueWriter output format**

**Source**: `crates/loro-internal/src/encoding/value.rs`

---

## Value Encoding

Loro uses a tagged value encoding system where each value is prefixed with a type tag.

### Value Kind Tags

| Tag | Kind | Description |
|-----|------|-------------|
| 0 | Null | Null value |
| 1 | True | Boolean true |
| 2 | False | Boolean false |
| 3 | I64 | 64-bit signed integer |
| 4 | F64 | 64-bit floating point |
| 5 | Str | String value |
| 6 | Binary | Binary data |
| 7 | ContainerType | Container type marker |
| 8 | DeleteOnce | Single deletion marker |
| 9 | DeleteSeq | Sequence deletion |
| 10 | DeltaInt | Delta-encoded integer |
| 11 | LoroValue | Nested LoroValue |
| 12 | MarkStart | Rich text mark start |
| 13 | TreeMove | Tree node move operation |
| 14 | ListMove | List move operation |
| 15 | ListSet | List set operation |
| 16 | RawTreeMove | Raw tree move (internal) |

**Source**: `crates/loro-internal/src/encoding/value.rs:39-165`

### Value Encoding Details

- [ ] **TODO: Document each value type's binary encoding**
  - I64 encoding (LEB128 or fixed?)
  - F64 encoding
  - String encoding (length-prefixed)
  - Binary encoding
  - MarkStart structure
  - TreeMove structure
  - ListMove structure

---

## Compression Techniques

Loro uses multiple compression techniques to minimize data size:

### LEB128 (Little Endian Base 128)

Variable-length encoding for unsigned integers. Used throughout the format for lengths and small integers.

```
Value < 128:        1 byte
Value < 16384:      2 bytes
Value < 2097152:    3 bytes
...
```

### Run-Length Encoding (RLE)

Used for sequences with repeated values:
- `BoolRle`: Compressed boolean sequences
- `AnyRle`: Generic RLE for any type

### Delta Encoding

Stores differences between consecutive values instead of absolute values:
- `DeltaRle`: Delta + RLE combination
- `DeltaOfDelta`: Second-order delta (for timestamps)

### Columnar Encoding (serde_columnar)

Operations are stored in columnar format for better compression:
- Groups same-type fields together
- Applies per-column compression strategies

### LZ4 Compression

Block-level compression for SSTable blocks:
- Fast compression and decompression
- Good compression ratio for structured data

---

## Appendix

### Endianness

- All multi-byte integers use **little-endian** byte order unless otherwise specified
- Exception: Encode Mode in header uses **big-endian**

### Checksums

| Location | Algorithm | Seed |
|----------|-----------|------|
| Document header | xxHash32 | 0x4F524F4C |
| SSTable Block Meta | xxHash32 | 0x4F524F4C |

### Constants

```rust
const MAGIC_BYTES: [u8; 4] = *b"loro";           // Document magic
const SSTABLE_MAGIC: [u8; 4] = *b"LORO";         // SSTable magic
const XXH_SEED: u32 = 0x4F524F4C;                // "LORO" as u32 LE
const MAX_BLOCK_SIZE: usize = 4096;              // ~4KB per change block
const DEFAULT_SSTABLE_BLOCK_SIZE: usize = 4096; // SSTable block size
```

### File Locations Reference

| Component | Source File |
|-----------|-------------|
| Main encoding module | `crates/loro-internal/src/encoding.rs` |
| FastSnapshot format | `crates/loro-internal/src/encoding/fast_snapshot.rs` |
| Shallow snapshot | `crates/loro-internal/src/encoding/shallow_snapshot.rs` |
| Change block encoding | `crates/loro-internal/src/oplog/change_store/block_encode.rs` |
| Block meta encoding | `crates/loro-internal/src/oplog/change_store/block_meta_encode.rs` |
| Value encoding | `crates/loro-internal/src/encoding/value.rs` |
| SSTable format | `crates/kv-store/src/sstable.rs` |
| Block format | `crates/kv-store/src/block.rs` |
| Container arena | `crates/loro-internal/src/encoding/arena.rs` |

---

## Checklist for Implementation

To implement a complete Loro decoder/encoder, you need to handle:

- [x] Document header parsing (magic, checksum, mode)
- [x] FastSnapshot body structure
- [x] FastUpdates body structure
- [x] SSTable parsing (header, blocks, meta)
- [ ] SSTable Block Chunk internal format
- [ ] LZ4 decompression for blocks
- [ ] VersionVector encoding/decoding
- [ ] Frontiers encoding/decoding
- [ ] ContainerID encoding/decoding
- [ ] ContainerWrapper encoding/decoding
- [ ] Change Block full parsing
- [ ] ContainerArena encoding/decoding
- [ ] PositionArena encoding/decoding (FractionalIndex)
- [ ] Value encoding/decoding for all types
- [ ] serde_columnar compatible decoder
- [ ] LEB128 encoder/decoder
- [ ] BoolRle encoder/decoder
- [ ] DeltaRle encoder/decoder
- [ ] DeltaOfDelta encoder/decoder
- [ ] Operation content parsing per container type
