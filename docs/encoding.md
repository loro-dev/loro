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
│ 4             │ Magic Bytes: "LORO" [0x4C, 0x4F, 0x52, 0x4F]     │
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
│               │ Covers all block meta entries                   │
└───────────────┴─────────────────────────────────────────────────┘
```

### Compression Types

| Value | Type |
|-------|------|
| 0 | None |
| 1 | LZ4 |

**Source**: `crates/kv-store/src/sstable.rs:23-141`

### Block Chunk Format

SSTable uses two types of blocks: Normal blocks and Large value blocks.

#### Normal Block Format

```
┌────────────────────────────────────────────────────────────────────────────┐
│ Normal Block (possibly LZ4 compressed body + uncompressed checksum)        │
├────────────────────────────────────────────────────────────────────────────┤
│ ┌─────────────────┬─────────────────┬─────────────────┬─────────────────┐  │
│ │ KV Chunk 1      │ KV Chunk 2      │ ...             │ KV Chunk N      │  │
│ └─────────────────┴─────────────────┴─────────────────┴─────────────────┘  │
│ ┌─────────────────┬─────────────────┬─────────────────┬─────────────────┐  │
│ │ offset[1] u16   │ offset[2] u16   │ ...             │ offset[N] u16   │  │
│ └─────────────────┴─────────────────┴─────────────────┴─────────────────┘  │
│ ┌─────────────────┐                                                        │
│ │ kv_count u16    │  (number of key-value pairs)                           │
│ └─────────────────┘                                                        │
│ ┌─────────────────┐                                                        │
│ │ checksum u32    │  (xxHash32 of compressed body, uncompressed)           │
│ └─────────────────┘                                                        │
└────────────────────────────────────────────────────────────────────────────┘
```

#### Key-Value Chunk Format

The first entry stores full key + value. Subsequent entries use prefix compression:

```
First KV Chunk:
┌─────────────────────────────────────────────────────────────────┐
│ value bytes (no key stored, key is in Block Meta first_key)    │
└─────────────────────────────────────────────────────────────────┘

Subsequent KV Chunks:
┌─────────────────────────────────────────────────────────────────┐
│ common_prefix_len: u8   │ Length of shared prefix with first_key│
├─────────────────────────┼───────────────────────────────────────┤
│ key_suffix_len: u16 LE  │ Length of key suffix                  │
├─────────────────────────┼───────────────────────────────────────┤
│ key_suffix: bytes       │ Key bytes after common prefix         │
├─────────────────────────┼───────────────────────────────────────┤
│ value: bytes            │ Value bytes (to next offset or end)   │
└─────────────────────────┴───────────────────────────────────────┘
```

#### Large Value Block Format

For values exceeding block size (~4KB):

```
┌─────────────────────────────────────────────────────────────────┐
│ Large Value Block                                                │
├───────────────┬─────────────────────────────────────────────────┤
│ variable      │ value bytes (possibly LZ4 compressed)           │
├───────────────┼─────────────────────────────────────────────────┤
│ 4             │ checksum u32 (xxHash32, little-endian)          │
└───────────────┴─────────────────────────────────────────────────┘
```

**Source**: `crates/kv-store/src/block.rs:21-323`

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
│ 0..8          │ PeerID (u64 big-endian)                         │
├───────────────┼─────────────────────────────────────────────────┤
│ 8..12         │ Counter (i32 big-endian) - start of block       │
└───────────────┴─────────────────────────────────────────────────┘
```

### VersionVector Encoding

VersionVector is encoded using **postcard** format (a compact binary serialization format):

```
┌─────────────────────────────────────────────────────────────────┐
│                    VersionVector Encoding                        │
├───────────────┬─────────────────────────────────────────────────┤
│ Bytes         │ Content                                         │
├───────────────┼─────────────────────────────────────────────────┤
│ LEB128        │ Number of entries (N)                           │
├───────────────┼─────────────────────────────────────────────────┤
│ For each entry:                                                 │
│   varint      │   PeerID (u64, postcard varint)                 │
│   varint      │   Counter (i32, postcard zigzag varint)         │
└───────────────┴─────────────────────────────────────────────────┘
```

The VersionVector is a HashMap<PeerID, Counter> serialized with postcard.

**Source**: `crates/loro-internal/src/version.rs:843-850`

### Frontiers Encoding

Frontiers is encoded as a sorted Vec<ID> using **postcard** format:

```
┌─────────────────────────────────────────────────────────────────┐
│                      Frontiers Encoding                          │
├───────────────┬─────────────────────────────────────────────────┤
│ Bytes         │ Content                                         │
├───────────────┼─────────────────────────────────────────────────┤
│ LEB128        │ Number of IDs (N)                               │
├───────────────┼─────────────────────────────────────────────────┤
│ For each ID (sorted):                                           │
│   varint      │   PeerID (u64, postcard encoding)               │
│   varint      │   Counter (i32, postcard zigzag encoding)       │
└───────────────┴─────────────────────────────────────────────────┘
```

**Source**: `crates/loro-internal/src/version/frontiers.rs:219-231`

---

## State Encoding

The document state is stored as a KV Store where each container's state is a separate entry.

### State KV Schema

| Key | Value |
|-----|-------|
| `ContainerID.to_bytes()` | ContainerWrapper encoded bytes |
| `FRONTIERS_KEY` | Frontiers encoding (for shallow snapshots) |

### ContainerID Encoding

```
┌─────────────────────────────────────────────────────────────────┐
│                    ContainerID Encoding                          │
├───────────────┬─────────────────────────────────────────────────┤
│ Root Container:                                                  │
├───────────────┼─────────────────────────────────────────────────┤
│ 1             │ first_byte: container_type | 0x80 (high bit set)│
│ LEB128        │ name length                                     │
│ variable      │ name (UTF-8 bytes)                              │
├───────────────┼─────────────────────────────────────────────────┤
│ Normal Container:                                                │
├───────────────┼─────────────────────────────────────────────────┤
│ 1             │ first_byte: container_type (high bit clear)     │
│ 8             │ PeerID (u64 little-endian)                      │
│ 4             │ Counter (i32 little-endian)                     │
└───────────────┴─────────────────────────────────────────────────┘

Container Types:
  0 = Map
  1 = List
  2 = Text
  3 = Tree
  4 = MovableList
  5 = Counter (if feature enabled)
```

**Source**: `crates/loro-common/src/lib.rs:193-254, 336-347`

### ContainerWrapper Encoding

Each container's state is wrapped in a ContainerWrapper:

```
┌─────────────────────────────────────────────────────────────────┐
│                   ContainerWrapper Encoding                      │
├───────────────┬─────────────────────────────────────────────────┤
│ 1             │ ContainerType (u8)                              │
├───────────────┼─────────────────────────────────────────────────┤
│ LEB128        │ Depth in container hierarchy                    │
├───────────────┼─────────────────────────────────────────────────┤
│ variable      │ Parent ContainerID (postcard Option<ContainerID>│
├───────────────┼─────────────────────────────────────────────────┤
│ variable      │ Container State Snapshot (type-specific)        │
└───────────────┴─────────────────────────────────────────────────┘
```

**Source**: `crates/loro-internal/src/state/container_store/container_wrapper.rs:100-120`

---

## Change Block Encoding

Each Change Block contains multiple consecutive Changes from the same peer. Blocks are approximately 4KB after compression.

**Source**: `crates/loro-internal/src/oplog/change_store/block_encode.rs:1-65`

### Block Structure Overview

The block is encoded using **postcard** serialization with the following structure:

```
┌──────────────────────────────────────────────────────────────────┐
│                       Change Block Layout                         │
├──────────────────────────────────────────────────────────────────┤
│ EncodedBlock (postcard serialized):                              │
│   counter_start: u32 (LEB128)                                    │
│   counter_len: u32 (LEB128)                                      │
│   lamport_start: u32 (LEB128)                                    │
│   lamport_len: u32 (LEB128)                                      │
│   n_changes: u32 (LEB128)                                        │
│   header: bytes (length-prefixed)                                │
│   change_meta: bytes (length-prefixed)                           │
│   cids: bytes (length-prefixed)                                  │
│   keys: bytes (length-prefixed)                                  │
│   positions: bytes (length-prefixed)                             │
│   ops: bytes (length-prefixed)                                   │
│   delete_start_ids: bytes (length-prefixed)                      │
│   values: bytes (length-prefixed)                                │
└──────────────────────────────────────────────────────────────────┘
```

### Header Section Detail

The `header` field contains change metadata:

```
┌──────────────────────────────────────────────────────────────────┐
│                      Header Field Layout                          │
├───────────────┬──────────────────────────────────────────────────┤
│ LEB128        │ peer_num (number of unique peers)                │
├───────────────┼──────────────────────────────────────────────────┤
│ 8 × peer_num  │ PeerIDs (u64 little-endian each)                 │
├───────────────┼──────────────────────────────────────────────────┤
│ N × LEB128    │ Change atom lengths (N-1 entries, last inferred) │
├───────────────┼──────────────────────────────────────────────────┤
│ BoolRle       │ dep_on_self flags (N entries)                    │
├───────────────┼──────────────────────────────────────────────────┤
│ AnyRle<usize> │ dependency lengths (N entries)                   │
├───────────────┼──────────────────────────────────────────────────┤
│ AnyRle<u32>   │ dep peer indices                                 │
├───────────────┼──────────────────────────────────────────────────┤
│ DeltaOfDelta  │ dep counters                                     │
├───────────────┼──────────────────────────────────────────────────┤
│ DeltaOfDelta  │ lamport timestamps (N-1 entries)                 │
└───────────────┴──────────────────────────────────────────────────┘
```

### Change Meta Section

```
┌──────────────────────────────────────────────────────────────────┐
│                    Change Meta Field Layout                       │
├───────────────┬──────────────────────────────────────────────────┤
│ DeltaOfDelta  │ Timestamps (N entries, i64)                      │
├───────────────┼──────────────────────────────────────────────────┤
│ AnyRle<u32>   │ Commit message lengths (N entries)               │
├───────────────┼──────────────────────────────────────────────────┤
│ Raw bytes     │ Commit messages (concatenated UTF-8 strings)     │
└───────────────┴──────────────────────────────────────────────────┘
```

**Source**: `crates/loro-internal/src/oplog/change_store/block_meta_encode.rs`

### Operations Section (serde_columnar)

Operations are encoded using `serde_columnar` for columnar storage:

```
┌──────────────────────────────────────────────────────────────────┐
│              Operations Encoding (serde_columnar)                 │
├───────────────┬──────────────────────────────────────────────────┤
│ Column        │ Strategy                                         │
├───────────────┼──────────────────────────────────────────────────┤
│ container_idx │ u32, DeltaRle strategy                           │
│ prop          │ i32, DeltaRle strategy                           │
│ value_type    │ u8, Rle strategy                                 │
│ len           │ u32, Rle strategy                                │
└───────────────┴──────────────────────────────────────────────────┘
```

### Container Arena Encoding (serde_columnar)

ContainerIDs are encoded in columnar format:

```
┌──────────────────────────────────────────────────────────────────┐
│                  ContainerArena (serde_columnar)                  │
├───────────────┬──────────────────────────────────────────────────┤
│ Column        │ Strategy                                         │
├───────────────┼──────────────────────────────────────────────────┤
│ is_root       │ bool, BoolRle strategy                           │
│ kind          │ u8, Rle strategy                                 │
│ peer_idx      │ usize, Rle strategy                              │
│ key_idx_or_counter│ i32, DeltaRle strategy                       │
└───────────────┴──────────────────────────────────────────────────┘
```

**Source**: `crates/loro-internal/src/encoding/arena.rs:39-50`

### Position Arena Encoding (serde_columnar)

Fractional index positions use prefix compression:

```
┌──────────────────────────────────────────────────────────────────┐
│                  PositionArena (serde_columnar)                   │
├───────────────┬──────────────────────────────────────────────────┤
│ Column        │ Strategy                                         │
├───────────────┼──────────────────────────────────────────────────┤
│ common_prefix │ usize, Rle strategy                              │
│ rest          │ bytes (Cow<[u8]>)                                │
└───────────────┴──────────────────────────────────────────────────┘

Decoding: For each position, take common_prefix bytes from previous
position and append rest bytes.
```

**Source**: `crates/loro-internal/src/encoding/arena.rs:159-232`

### Key Strings Section

```
┌──────────────────────────────────────────────────────────────────┐
│                    Key Strings Encoding                           │
├───────────────┬──────────────────────────────────────────────────┤
│ For each key:                                                    │
│   LEB128      │   key length                                     │
│   bytes       │   UTF-8 encoded key string                       │
└───────────────┴──────────────────────────────────────────────────┘
```

### Delete Start IDs Section (serde_columnar)

```
┌──────────────────────────────────────────────────────────────────┐
│              Delete Start IDs (serde_columnar)                    │
├───────────────┬──────────────────────────────────────────────────┤
│ Column        │ Strategy                                         │
├───────────────┼──────────────────────────────────────────────────┤
│ peer_idx      │ u32, DeltaRle                                    │
│ counter       │ i32, DeltaRle                                    │
│ len           │ i32, Rle                                         │
└───────────────┴──────────────────────────────────────────────────┘
```

---

## Value Encoding

Loro uses a tagged value encoding system where each value is prefixed with a type tag.

### Value Kind Tags

| Tag | Kind | Description |
|-----|------|-------------|
| 0 | Null | Null value |
| 1 | True | Boolean true |
| 2 | False | Boolean false |
| 3 | I64 | 64-bit signed integer (LEB128 signed) |
| 4 | F64 | 64-bit floating point (big-endian) |
| 5 | Str | String value (LEB128 len + UTF-8 bytes) |
| 6 | Binary | Binary data (LEB128 len + bytes) |
| 7 | ContainerType | Container type marker (LEB128 index) |
| 8 | DeleteOnce | Single deletion marker |
| 9 | DeleteSeq | Sequence deletion |
| 10 | DeltaInt | Delta-encoded i32 (LEB128 signed) |
| 11 | LoroValue | Nested LoroValue |
| 12 | MarkStart | Rich text mark start |
| 13 | TreeMove | Tree node move operation |
| 14 | ListMove | List move operation |
| 15 | ListSet | List set operation |
| 16 | RawTreeMove | Raw tree move (internal) |
| 0x80+ | Future | Unknown/future value types |

**Source**: `crates/loro-internal/src/encoding/value.rs:39-161`

### Value Encoding Details

#### Primitive Types

```
Null:      (no data)
True:      (no data)
False:     (no data)
I64:       LEB128 signed encoding
F64:       8 bytes big-endian IEEE 754
Str:       LEB128(len) + UTF-8 bytes
Binary:    LEB128(len) + raw bytes
DeltaInt:  LEB128 signed encoding (i32)
```

#### LoroValue (Nested Value)

LoroValue can contain nested structures:

```
┌──────────────────────────────────────────────────────────────────┐
│                    LoroValue Encoding                             │
├───────────────┬──────────────────────────────────────────────────┤
│ 1             │ LoroValueKind tag (u8)                           │
├───────────────┼──────────────────────────────────────────────────┤
│ variable      │ Value content (depends on kind)                  │
└───────────────┴──────────────────────────────────────────────────┘

LoroValueKind:
  0 = Null
  1 = True
  2 = False
  3 = I64 (LEB128 signed)
  4 = F64 (8 bytes BE)
  5 = Str (LEB128 len + UTF-8)
  6 = Binary (LEB128 len + bytes)
  7 = List (LEB128 len + N × LoroValue)
  8 = Map (LEB128 len + N × (LEB128 key_idx + LoroValue))
  9 = ContainerType (u8)
```

#### MarkStart (Rich Text)

```
┌──────────────────────────────────────────────────────────────────┐
│                    MarkStart Encoding                             │
├───────────────┬──────────────────────────────────────────────────┤
│ 1             │ info (u8 flags)                                  │
│ LEB128        │ len (mark length)                                │
│ LEB128        │ key_idx (index into keys array)                  │
│ variable      │ value (LoroValue with type+content)              │
└───────────────┴──────────────────────────────────────────────────┘
```

#### TreeMove

```
┌──────────────────────────────────────────────────────────────────┐
│                    TreeMove Encoding                              │
├───────────────┬──────────────────────────────────────────────────┤
│ LEB128        │ target_idx (index into tree_ids)                 │
│ 1             │ is_parent_null (u8 as bool)                      │
│ LEB128        │ position (index into positions)                  │
│ LEB128        │ parent_idx (only if !is_parent_null)             │
└───────────────┴──────────────────────────────────────────────────┘
```

#### ListMove

```
┌──────────────────────────────────────────────────────────────────┐
│                    ListMove Encoding                              │
├───────────────┬──────────────────────────────────────────────────┤
│ LEB128        │ from (source position)                           │
│ LEB128        │ from_idx                                         │
│ LEB128        │ lamport                                          │
└───────────────┴──────────────────────────────────────────────────┘
```

#### ListSet

```
┌──────────────────────────────────────────────────────────────────┐
│                    ListSet Encoding                               │
├───────────────┬──────────────────────────────────────────────────┤
│ LEB128        │ peer_idx                                         │
│ LEB128        │ lamport                                          │
│ variable      │ value (LoroValue with type+content)              │
└───────────────┴──────────────────────────────────────────────────┘
```

**Source**: `crates/loro-internal/src/encoding/value.rs:343-458, 858-990`

---

## Compression Techniques

Loro uses multiple compression techniques to minimize data size.

### LEB128 (Little Endian Base 128)

Variable-length encoding for unsigned integers:

```
Value Range          Bytes
0-127               1
128-16383           2
16384-2097151       3
...

Encoding:
- Take 7 bits at a time from LSB
- Set high bit (0x80) if more bytes follow
- Last byte has high bit clear

For signed integers, use zigzag encoding first:
  encoded = (n << 1) ^ (n >> 63)
```

### Run-Length Encoding (RLE)

#### BoolRle

Encodes sequences of booleans as runs:

```
┌──────────────────────────────────────────────────────────────────┐
│                      BoolRle Format                               │
├───────────────┬──────────────────────────────────────────────────┤
│ Encoding:     │ Alternating run lengths starting with true      │
│ LEB128        │ Count of true values                            │
│ LEB128        │ Count of false values                           │
│ LEB128        │ Count of true values                            │
│ ...           │ (continues alternating)                         │
└───────────────┴──────────────────────────────────────────────────┘
```

#### AnyRle<T>

Generic RLE for any type with equality:

```
┌──────────────────────────────────────────────────────────────────┐
│                      AnyRle Format                                │
├───────────────┬──────────────────────────────────────────────────┤
│ For each run: │                                                  │
│   T           │   Value (encoded per type)                       │
│   LEB128      │   Run length                                     │
└───────────────┴──────────────────────────────────────────────────┘
```

### Delta Encoding

#### DeltaRle

Stores differences between consecutive values with RLE:

```
┌──────────────────────────────────────────────────────────────────┐
│                     DeltaRle Format                               │
├───────────────┬──────────────────────────────────────────────────┤
│ LEB128        │ First value                                      │
│ For each run: │                                                  │
│   LEB128 signed│   Delta from previous                           │
│   LEB128      │   Run length                                     │
└───────────────┴──────────────────────────────────────────────────┘
```

#### DeltaOfDelta

Second-order delta encoding (differences of differences):

```
┌──────────────────────────────────────────────────────────────────┐
│                   DeltaOfDelta Format                             │
├───────────────┬──────────────────────────────────────────────────┤
│ LEB128 signed │ First value                                      │
│ LEB128 signed │ First delta                                      │
│ For each:     │                                                  │
│   LEB128 signed│   Delta of delta                                │
└───────────────┴──────────────────────────────────────────────────┘

Decoding:
  value[0] = first_value
  delta = first_delta
  for dod in delta_of_deltas:
    delta += dod
    value[i] = value[i-1] + delta
```

Useful for monotonically increasing sequences like timestamps.

**Source**: `crates/loro-internal/src/oplog/change_store/block_meta_encode.rs:1-184`

### Columnar Encoding (serde_columnar)

The `serde_columnar` library provides columnar storage with per-column compression:

```
┌──────────────────────────────────────────────────────────────────┐
│                  serde_columnar Format                            │
├───────────────┬──────────────────────────────────────────────────┤
│ For each column:                                                 │
│   LEB128      │   Column data length                             │
│   bytes       │   Column data (strategy-dependent)               │
│               │                                                  │
│ Strategies:   │                                                  │
│   Rle         │   Run-length encoded values                      │
│   DeltaRle    │   Delta + RLE                                    │
│   BoolRle     │   Boolean RLE                                    │
│   Raw         │   No compression                                 │
└───────────────┴──────────────────────────────────────────────────┘
```

### LZ4 Compression

Block-level compression for SSTable blocks:

- Applied to the entire block body (before checksum)
- Falls back to uncompressed if compression increases size
- Checksum is always uncompressed

---

## Appendix

### Endianness

- All multi-byte integers use **little-endian** byte order unless otherwise specified
- Exception: Encode Mode in header uses **big-endian**
- Exception: F64 values use **big-endian** (IEEE 754)
- Exception: ID.to_bytes() uses **big-endian** for both peer and counter

### Checksums

| Location | Algorithm | Seed |
|----------|-----------|------|
| Document header | xxHash32 | 0x4F524F4C |
| SSTable Block Meta | xxHash32 | 0x4F524F4C |
| SSTable Block | xxHash32 | 0x4F524F4C |

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
| Arena encoding | `crates/loro-internal/src/encoding/arena.rs` |
| SSTable format | `crates/kv-store/src/sstable.rs` |
| Block format | `crates/kv-store/src/block.rs` |
| ContainerID | `crates/loro-common/src/lib.rs` |
| ContainerWrapper | `crates/loro-internal/src/state/container_store/container_wrapper.rs` |
| VersionVector | `crates/loro-internal/src/version.rs` |
| Frontiers | `crates/loro-internal/src/version/frontiers.rs` |

---

## Checklist for Implementation

To implement a complete Loro decoder/encoder, you need to handle:

- [x] Document header parsing (magic, checksum, mode)
- [x] FastSnapshot body structure
- [x] FastUpdates body structure
- [x] SSTable parsing (header, blocks, meta)
- [x] SSTable Block Chunk internal format (Normal + Large)
- [x] LZ4 decompression for blocks
- [x] VersionVector encoding/decoding (postcard)
- [x] Frontiers encoding/decoding (postcard)
- [x] ContainerID encoding/decoding
- [x] ContainerWrapper encoding/decoding
- [x] Change Block full parsing
- [x] ContainerArena encoding/decoding (serde_columnar)
- [x] PositionArena encoding/decoding (prefix compression)
- [x] Value encoding/decoding for all types
- [x] serde_columnar compatible decoder
- [x] LEB128 encoder/decoder (unsigned and signed/zigzag)
- [x] BoolRle encoder/decoder
- [x] AnyRle encoder/decoder
- [x] DeltaRle encoder/decoder
- [x] DeltaOfDelta encoder/decoder
- [x] Operation content parsing per container type
