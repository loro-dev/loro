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
- [Supplementary Documentation](#supplementary-documentation)

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

> **See Also**: [xxHash32 Algorithm Specification](./encoding-xxhash32.md) for complete implementation details and JavaScript code.

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

**Source**: `crates/loro-common/src/lib.rs:42-47` (ID.to_bytes)

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

> **See Also**: [Container State Snapshot Encoding](./encoding-container-states.md) for detailed encoding formats of Map, List, Text, Tree, and MovableList containers.

### State KV Schema

| Key | Value |
|-----|-------|
| `ContainerID.to_bytes()` | ContainerWrapper encoded bytes |
| `FRONTIERS_KEY` (`b"fr"`) | Frontiers encoding (for shallow snapshots) |

**Source**: `crates/loro-internal/src/state/container_store.rs:48` (FRONTIERS_KEY)

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

**Source**: `crates/loro-internal/src/oplog/change_store/block_encode.rs:414-428` (EncodedOp struct)

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

**Source**: `crates/loro-internal/src/encoding/value.rs:860-867` (read_str/write_str pattern)

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

**Source**: `crates/loro-internal/src/encoding/outdated_encode_reordered.rs:429-433` (EncodedDeleteStartId)

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

**Source**: `crates/loro-internal/src/encoding/value.rs:858-882` (read_i64, read_f64)
**Source**: `crates/loro-internal/src/encoding/value.rs:1045-1073` (write_i64, write_f64)

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

**Source**: `crates/loro-internal/src/encoding/value.rs:63-85` (LoroValueKind enum)
**Source**: `crates/loro-internal/src/encoding/value.rs:620-660` (decode_loro_value)

#### MarkStart (Rich Text)

```
┌──────────────────────────────────────────────────────────────────┐
│                    MarkStart Encoding                             │
├───────────────┬──────────────────────────────────────────────────┤
│ 1             │ info (u8 flags, see below)                       │
│ LEB128        │ len (mark length in characters)                  │
│ LEB128        │ key_idx (index into keys array)                  │
│ variable      │ value (LoroValue with type+content)              │
└───────────────┴──────────────────────────────────────────────────┘

Info byte bit layout:
┌───────────────────────────────────────────────────────────────────┐
│ Bit 7 │ Bit 6-3 │ Bit 2         │ Bit 1         │ Bit 0          │
│ ALIVE │ Reserved│ EXPAND_AFTER  │ EXPAND_BEFORE │ Reserved       │
│ (0x80)│ (0)     │ (0x04)        │ (0x02)        │ (0)            │
└───────────────────────────────────────────────────────────────────┘

- ALIVE (0x80): Always set for active styles
- EXPAND_BEFORE (0x02): Style expands when text inserted before
- EXPAND_AFTER (0x04): Style expands when text inserted after

Common patterns:
- Bold style:    0x84 (ALIVE | EXPAND_AFTER)
- Link style:    0x80 (ALIVE only, no expansion)
- Comment style: 0x80 (ALIVE only, no expansion)
```

**Source**: `crates/loro-internal/src/encoding/value.rs:462-468` (MarkStart struct)
**Source**: `crates/loro-internal/src/encoding/value.rs:937-950` (read_mark)
**Source**: `crates/loro-internal/src/container/richtext.rs:141-282` (TextStyleInfoFlag)

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

**Source**: `crates/loro-internal/src/encoding/value.rs:480-485` (EncodedTreeMove struct)
**Source**: `crates/loro-internal/src/encoding/value.rs:953-967` (read_tree_move)

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

**Source**: `crates/loro-internal/src/encoding/value.rs:181-185` (ListMove variant)
**Source**: `crates/loro-internal/src/encoding/value.rs:367-375` (read ListMove)

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

**Source**: `crates/loro-internal/src/encoding/value.rs:186-190` (ListSet variant)
**Source**: `crates/loro-internal/src/encoding/value.rs:377-385` (read ListSet)

---

## Compression Techniques

Loro uses multiple compression techniques to minimize data size.

### LEB128 (Little Endian Base 128)

Variable-length encoding for integers. Used throughout Loro's custom value encoding.

#### Unsigned LEB128

```
Value Range          Bytes
0-127               1
128-16383           2
16384-2097151       3
...

Encoding Algorithm (JavaScript):
function encodeULEB128(value) {
  const bytes = [];
  do {
    let byte = value & 0x7F;      // Take low 7 bits
    value >>>= 7;                  // Shift right by 7
    if (value !== 0) {
      byte |= 0x80;                // Set continuation bit
    }
    bytes.push(byte);
  } while (value !== 0);
  return bytes;
}

Decoding Algorithm (JavaScript):
function decodeULEB128(bytes, offset = 0) {
  let result = 0;
  let shift = 0;
  let byte;
  do {
    byte = bytes[offset++];
    result |= (byte & 0x7F) << shift;
    shift += 7;
  } while (byte & 0x80);
  return { value: result, bytesRead: offset };
}

Examples:
  0     → [0x00]
  1     → [0x01]
  127   → [0x7F]
  128   → [0x80, 0x01]
  300   → [0xAC, 0x02]
  16384 → [0x80, 0x80, 0x01]
```

#### Signed LEB128 (SLEB128)

Loro uses standard signed LEB128 for signed integers (I64, DeltaInt, etc.),
which uses two's complement representation with sign extension.

```
Encoding Algorithm (JavaScript):
function encodeSLEB128(value) {
  const bytes = [];
  let more = true;
  while (more) {
    let byte = value & 0x7F;
    value >>= 7;  // Arithmetic shift (sign-extending)

    // Check if we need more bytes
    const signBit = byte & 0x40;
    if ((value === 0 && !signBit) || (value === -1 && signBit)) {
      more = false;
    } else {
      byte |= 0x80;  // Set continuation bit
    }
    bytes.push(byte);
  }
  return bytes;
}

Decoding Algorithm (JavaScript):
function decodeSLEB128(bytes, offset = 0) {
  let result = 0;
  let shift = 0;
  let byte;
  do {
    byte = bytes[offset++];
    result |= (byte & 0x7F) << shift;
    shift += 7;
  } while (byte & 0x80);

  // Sign extend if the sign bit (bit 6 of last byte) is set
  if (shift < 32 && (byte & 0x40)) {
    result |= (~0 << shift);  // Sign extend
  }
  return { value: result, bytesRead: offset };
}

Examples:
   0    → [0x00]
   1    → [0x01]
  -1    → [0x7F]
   63   → [0x3F]
  -64   → [0x40]
   64   → [0xC0, 0x00]
  -65   → [0xBF, 0x7F]
  127   → [0xFF, 0x00]
  -128  → [0x80, 0x7F]
```

**Important**: SLEB128 is different from zigzag encoding used by postcard.
- SLEB128: -1 encodes as [0x7F] (two's complement sign extension)
- Zigzag:  -1 encodes as [0x01] (maps -1 → 1, then varint)

**Source**: `crates/loro-internal/src/encoding/value.rs:858-859` (leb128::read::signed)
**Source**: `crates/loro-internal/src/encoding/value.rs:1046-1047` (leb128::write::signed)

### Run-Length Encoding (RLE)

#### BoolRle

Encodes sequences of booleans as alternating run lengths. The encoder starts
in the "false" state, so the first count is always for `false` values (which
can be 0 if the sequence starts with `true`).

```
┌──────────────────────────────────────────────────────────────────┐
│                      BoolRle Format                               │
├───────────────┬──────────────────────────────────────────────────┤
│ varint        │ Count of initial false values (can be 0)        │
│ varint        │ Count of following true values                  │
│ varint        │ Count of following false values                 │
│ ...           │ (continues alternating until all values covered)│
└───────────────┴──────────────────────────────────────────────────┘

Examples:
- [T, T, F, F, F]        → [0x00, 0x02, 0x03]  (0 false, 2 true, 3 false)
- [F, F, F, T, T]        → [0x03, 0x02]        (3 false, 2 true)
- [T, T, T, T, T]        → [0x00, 0x05]        (0 false, 5 true)
- [F, F, F]              → [0x03]              (3 false)
- [T, T, T, F, F, T]     → [0x00, 0x03, 0x02, 0x01] (0 false, 3 true, 2 false, 1 true)
```

**Source**: `crates/loro-internal/src/oplog/change_store/block_meta_encode.rs:22,121` (BoolRleEncoder/Decoder usage)

#### AnyRle<T>

Generic RLE for any type with equality. Used in change block headers.
Supports both run-length encoding (repeated values) and literal runs (distinct values).

```
┌──────────────────────────────────────────────────────────────────┐
│                      AnyRle Format                                │
├───────────────┬──────────────────────────────────────────────────┤
│ For each segment:                                                │
│   zigzag varint│  Signed length (positive=run, negative=literal)│
│   T (postcard) │  Value(s) - 1 if run, |length| if literal      │
├───────────────┼──────────────────────────────────────────────────┤
│ Run (len > 0):│  Length N followed by 1 value, repeated N times │
│ Literal (len<0):│ Length -N followed by N distinct values       │
└───────────────┴──────────────────────────────────────────────────┘

Zigzag encoding for length (isize):
  positive N → zigzag(N) = N * 2
  negative N → zigzag(N) = (-N * 2) - 1

Example: AnyRle<u64> encoding [5, 5, 5, 2, 2]
  - Run 1: length=3 → zigzag(3)=6 → [0x06], value=5 → [0x05]
  - Run 2: length=2 → zigzag(2)=4 → [0x04], value=2 → [0x02]
  - Full:  [0x06, 0x05, 0x04, 0x02]

Example: AnyRle<u32> encoding [1, 2, 3] (all different - literal run)
  - Literal: length=-3 → zigzag(-3)=5 → [0x05], values=[0x01, 0x02, 0x03]
  - Full: [0x05, 0x01, 0x02, 0x03]
```

**Source**: `crates/loro-internal/src/oplog/change_store/block_meta_encode.rs:20,23,25` (AnyRleEncoder usage)

### Delta Encoding

#### DeltaRle

See the [DeltaRle Column Strategy](#deltarle-column-strategy) section under serde_columnar for
the complete format. DeltaRle applies AnyRle to delta values.

**Source**: Uses `serde_columnar::DeltaRle` strategy annotation

#### DeltaOfDelta

Second-order delta encoding with variable-width bit-packing. This is highly efficient for
monotonically increasing sequences like timestamps or counters.

```
┌──────────────────────────────────────────────────────────────────┐
│                   DeltaOfDelta Format                             │
├───────────────┬──────────────────────────────────────────────────┤
│ Header:                                                          │
│   postcard    │ Option<i64> - first value (None if empty)       │
│   1 byte      │ Number of valid bits in last byte (1-8)         │
├───────────────┼──────────────────────────────────────────────────┤
│ Bit-packed delta-of-deltas (big-endian bit order):               │
│                                                                  │
│ Prefix Code   │ Meaning                                         │
│ 0             │ Delta-of-delta = 0 (1 bit total)                │
│ 10 + 7 bits   │ Value in [-63, 64], stored as value+63          │
│ 110 + 9 bits  │ Value in [-255, 256], stored as value+255       │
│ 1110 + 12 bits│ Value in [-2047, 2048], stored as value+2047    │
│ 11110 + 21 bits│Value in [-(2^20)+1, 2^20], biased               │
│ 11111 + 64 bits│Full i64 value (two's complement)               │
└───────────────┴──────────────────────────────────────────────────┘

Decoding algorithm:
  1. Read postcard Option<i64> for first value (return if None)
  2. Read 1 byte for last_used_bits
  3. Read remaining bytes as bit stream (big-endian)
  4. For each value after first:
     - Read prefix bits to determine range
     - Read value bits and subtract bias
     - delta += delta_of_delta
     - value = prev_value + delta
     - yield value
```

Example: Encoding [1, 2, 3, 4, 5, 6]
- Values: 1, 2, 3, 4, 5, 6
- Deltas: 1, 1, 1, 1, 1 (all +1)
- Delta-of-deltas: 0, 0, 0, 0 (after first delta)
- Each 0 encodes as single "0" bit → 5 bits total for 6 values!

**Source**: `serde_columnar-0.3.14/src/strategy/rle.rs:405-513` (DeltaOfDeltaEncoder)
**Source**: `crates/loro-internal/src/oplog/change_store/block_meta_encode.rs:18-19,26` (usage)

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
- Uses LZ4 Frame format (not raw LZ4 block format)

> **See Also**: [LZ4 Frame Format Specification](./encoding-lz4.md) for complete format details and JavaScript implementation.

**Source**: `crates/kv-store/src/block.rs:89-112` (compress_block)
**Source**: `crates/kv-store/src/block.rs:193-212` (decompress_block)

---

## External Format Specifications

Loro uses two external serialization libraries: **postcard** and **serde_columnar**. For implementers
in other languages, this section provides the exact wire format specifications.

### Postcard Wire Format

Postcard is used for serializing VersionVector, Frontiers, and as the underlying serializer for
serde_columnar. The format is documented at https://postcard.jamesmunns.com/wire-format.html

#### Postcard Varint (Unsigned)

All unsigned integers larger than 1 byte use variable-length encoding:

```
┌──────────────────────────────────────────────────────────────────┐
│                    Postcard Unsigned Varint                       │
├───────────────┬──────────────────────────────────────────────────┤
│ Encoding:     │                                                  │
│   - Each byte stores 7 data bits + 1 continuation bit           │
│   - Continuation bit (MSB): 1 = more bytes, 0 = last byte       │
│   - Little-endian byte order (LSB first)                        │
│   - Max bytes limited by type (u16=3, u32=5, u64=10)            │
├───────────────┼──────────────────────────────────────────────────┤
│ Example:      │ 300 (0x12C) encodes as [0xAC, 0x02]             │
│               │   0xAC = 0b1_0101100 (cont=1, value=44)         │
│               │   0x02 = 0b0_0000010 (cont=0, value=2)          │
│               │   result = 44 + (2 << 7) = 44 + 256 = 300       │
└───────────────┴──────────────────────────────────────────────────┘
```

#### Postcard Varint (Signed) - Zigzag Encoding

Signed integers use zigzag encoding to make small negative numbers efficient:

```
┌──────────────────────────────────────────────────────────────────┐
│                  Postcard Signed Varint (Zigzag)                  │
├───────────────┬──────────────────────────────────────────────────┤
│ Step 1:       │ Zigzag encode: z = (n << 1) ^ (n >> (bits-1))   │
│               │   0 → 0,  -1 → 1,  1 → 2,  -2 → 3,  2 → 4, ...  │
├───────────────┼──────────────────────────────────────────────────┤
│ Step 2:       │ Encode z as unsigned varint                      │
├───────────────┼──────────────────────────────────────────────────┤
│ Decoding:     │ n = (z >> 1) ^ -(z & 1)                         │
├───────────────┼──────────────────────────────────────────────────┤
│ Examples:     │ -1_i32 → zigzag(1) → varint [0x01]              │
│               │  1_i32 → zigzag(2) → varint [0x02]              │
│               │ -64_i32 → zigzag(127) → varint [0x7F]           │
└───────────────┴──────────────────────────────────────────────────┘
```

#### Postcard Other Types

```
┌──────────────────────────────────────────────────────────────────┐
│                    Postcard Type Encodings                        │
├──────────────┬───────────────────────────────────────────────────┤
│ Type         │ Encoding                                          │
├──────────────┼───────────────────────────────────────────────────┤
│ bool         │ 0x00 = false, 0x01 = true                        │
│ u8 / i8      │ Single byte (i8 uses two's complement)           │
│ u16-u128     │ Varint (unsigned)                                │
│ i16-i128     │ Zigzag + Varint                                  │
│ f32          │ 4 bytes little-endian IEEE 754                   │
│ f64          │ 8 bytes little-endian IEEE 754                   │
│ Option<T>    │ 0x00 = None, 0x01 + T = Some(T)                  │
│ Vec<T>/[T]   │ varint(len) + N × T                              │
│ String/&str  │ varint(len) + UTF-8 bytes                        │
│ Tuple (A,B)  │ A followed by B (no length prefix)               │
│ Struct       │ Fields in declaration order (no field names)     │
│ Enum variant │ varint(discriminant) + variant data              │
│ HashMap<K,V> │ varint(len) + N × (K, V)                         │
└──────────────┴───────────────────────────────────────────────────┘
```

**Important Note**: Postcard's f64 uses **little-endian**, but Loro's custom value encoding uses
**big-endian** for F64. Be careful not to confuse them.

**Source**: https://postcard.jamesmunns.com/wire-format.html

### serde_columnar Wire Format

serde_columnar organizes struct fields into columns and applies per-column compression strategies.
It uses postcard as the underlying serializer.

#### Overall Structure

For a struct with fields marked as `#[columnar(vec, ...)]`:

```
┌──────────────────────────────────────────────────────────────────┐
│                   serde_columnar Vec Format                       │
├───────────────┬──────────────────────────────────────────────────┤
│ postcard varint│ Number of rows (N)                              │
├───────────────┼──────────────────────────────────────────────────┤
│ For each column (in field declaration order):                    │
│   postcard varint│ Column data length in bytes                   │
│   bytes       │   Column data (strategy-dependent encoding)      │
└───────────────┴──────────────────────────────────────────────────┘
```

#### BoolRle Column Strategy

Encodes boolean columns as run lengths, starting with the count of initial `false` values
(the encoder starts in the "false" state):

```
┌──────────────────────────────────────────────────────────────────┐
│                   BoolRle Column Encoding                         │
├───────────────┬──────────────────────────────────────────────────┤
│ Encoding:     │ Alternating run lengths of false/true values    │
├───────────────┼──────────────────────────────────────────────────┤
│ postcard varint│ Count of leading false values (can be 0)       │
│ postcard varint│ Count of following true values                  │
│ postcard varint│ Count of following false values                 │
│ ...           │ (continues alternating until all N values)       │
├───────────────┼──────────────────────────────────────────────────┤
│ Example:      │ [T, T, F, F, F, T] encodes as [0, 2, 3, 1]      │
│               │   0 false, 2 true, 3 false, 1 true               │
├───────────────┼──────────────────────────────────────────────────┤
│ Edge case:    │ [F, F, T] encodes as [2, 1]                     │
│               │   2 leading falses, 1 true                       │
└───────────────┴──────────────────────────────────────────────────┘
```

**Source**: `serde_columnar-0.3.14/src/strategy/rle.rs:15-43,248-309` (BoolRleEncoder/Decoder)

#### Rle Column Strategy

Run-length encoding for any comparable type. Uses AnyRle format with signed length
(supports both runs and literal sequences):

```
┌──────────────────────────────────────────────────────────────────┐
│                    Rle Column Encoding                            │
├───────────────┬──────────────────────────────────────────────────┤
│ For each segment:                                                │
│   zigzag varint│  Signed length (positive=run, negative=literal)│
│   T (postcard) │  Value(s) - see AnyRle format above            │
├───────────────┼──────────────────────────────────────────────────┤
│ Run (len > 0):│  Length N followed by 1 value, repeated N times │
│ Literal (len<0):│ Length -N followed by N distinct values       │
├───────────────┼──────────────────────────────────────────────────┤
│ Example:      │ [5, 5, 5, 3, 3] (u8 values)                     │
│               │   Run 1: zigzag(3)=6, value=5 → [0x06, 0x05]    │
│               │   Run 2: zigzag(2)=4, value=3 → [0x04, 0x03]    │
│               │   Full: [0x06, 0x05, 0x04, 0x03]                │
└───────────────┴──────────────────────────────────────────────────┘
```

**Source**: `serde_columnar-0.3.14/src/strategy/rle.rs:45-155,165-246` (AnyRleEncoder/Decoder)

#### DeltaRle Column Strategy

Computes deltas from consecutive values, then applies AnyRle<i128> to the deltas.
Values are implicitly reconstructed from an initial value of 0.

```
┌──────────────────────────────────────────────────────────────────┐
│                   DeltaRle Column Encoding                        │
├───────────────┬──────────────────────────────────────────────────┤
│ Encoding:     │ AnyRle<i128> applied to delta sequence          │
│               │ Delta[0] = value[0] - 0 = value[0]              │
│               │ Delta[i] = value[i] - value[i-1]                │
├───────────────┼──────────────────────────────────────────────────┤
│ For each delta segment (AnyRle format):                          │
│   zigzag varint│  Signed length (positive=run, negative=literal)│
│   i128 zigzag │  Delta value(s) - see AnyRle format             │
├───────────────┼──────────────────────────────────────────────────┤
│ Example:      │ [10, 11, 12, 13, 15, 17]                        │
│               │   Deltas: [10, 1, 1, 1, 2, 2]                   │
│               │   AnyRle: run(1, 10), run(3, 1), run(2, 2)      │
│               │   Bytes: [zigzag(1), zigzag(10),                │
│               │           zigzag(3), zigzag(1),                 │
│               │           zigzag(2), zigzag(2)]                 │
├───────────────┼──────────────────────────────────────────────────┤
│ Decoding:     │ absolute = 0                                    │
│               │ for each delta from AnyRle:                      │
│               │   absolute += delta                              │
│               │   yield absolute                                 │
└───────────────┴──────────────────────────────────────────────────┘
```

**Source**: `serde_columnar-0.3.14/src/strategy/rle.rs:311-401` (DeltaRleEncoder/Decoder)

#### Example: ContainerArena Encoding

Given the struct definition:

```rust
#[columnar(vec, ser, de)]
struct EncodedContainer {
    #[columnar(strategy = "BoolRle")]   is_root: bool,
    #[columnar(strategy = "Rle")]       kind: u8,
    #[columnar(strategy = "Rle")]       peer_idx: usize,
    #[columnar(strategy = "DeltaRle")]  key_idx_or_counter: i32,
}
```

The wire format for `Vec<EncodedContainer>` is:

```
┌──────────────────────────────────────────────────────────────────┐
│              ContainerArena Wire Format Example                   │
├───────────────┬──────────────────────────────────────────────────┤
│ varint        │ N (number of containers)                         │
├───────────────┼──────────────────────────────────────────────────┤
│ varint        │ is_root column byte length                       │
│ bytes         │ is_root data (BoolRle encoded)                   │
├───────────────┼──────────────────────────────────────────────────┤
│ varint        │ kind column byte length                          │
│ bytes         │ kind data (Rle encoded u8 values)                │
├───────────────┼──────────────────────────────────────────────────┤
│ varint        │ peer_idx column byte length                      │
│ bytes         │ peer_idx data (Rle encoded usize as varint)      │
├───────────────┼──────────────────────────────────────────────────┤
│ varint        │ key_idx_or_counter column byte length            │
│ bytes         │ key_idx_or_counter data (DeltaRle encoded i32)   │
└───────────────┴──────────────────────────────────────────────────┘
```

**Source**: https://github.com/loro-dev/columnar (serde_columnar crate)
**Source**: `crates/loro-internal/src/encoding/arena.rs:39-50` (ContainerArena definition)

---

## Appendix

### Endianness

- All multi-byte integers use **little-endian** byte order unless otherwise specified
- Exception: Encode Mode in header uses **big-endian**
- Exception: F64 values use **big-endian** (IEEE 754)
- Exception: ID.to_bytes() uses **big-endian** for both peer and counter

**Source**: `crates/loro-common/src/lib.rs:42-47` (ID.to_bytes - big-endian)
**Source**: `crates/loro-common/src/lib.rs:212-213` (ContainerID.encode - little-endian)
**Source**: `crates/loro-internal/src/encoding/value.rs:874-882` (F64 - big-endian)

### Checksums

| Location | Algorithm | Seed |
|----------|-----------|------|
| Document header | xxHash32 | 0x4F524F4C |
| SSTable Block Meta | xxHash32 | 0x4F524F4C |
| SSTable Block | xxHash32 | 0x4F524F4C |

**Source**: `crates/loro-internal/src/encoding.rs:275` (XXH_SEED)
**Source**: `crates/kv-store/src/sstable.rs:13` (XXH_SEED)
**Source**: `crates/kv-store/src/sstable.rs:87,100` (checksum calculation)

### Constants

```rust
const MAGIC_BYTES: [u8; 4] = *b"loro";           // Document magic
const SSTABLE_MAGIC: [u8; 4] = *b"LORO";         // SSTable magic
const XXH_SEED: u32 = 0x4F524F4C;                // "LORO" as u32 LE
const MAX_BLOCK_SIZE: usize = 4096;              // ~4KB per change block
const DEFAULT_SSTABLE_BLOCK_SIZE: usize = 4096; // SSTable block size
```

**Source**: `crates/loro-internal/src/encoding.rs:269-275` (MAGIC_BYTES, XXH_SEED)
**Source**: `crates/kv-store/src/sstable.rs:13-14` (XXH_SEED, MAGIC_BYTES)
**Source**: `crates/loro-internal/src/oplog/change_store.rs:40` (MAX_BLOCK_SIZE)

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

## Supplementary Documentation

The following supplementary documents provide detailed specifications for components that require significant implementation effort:

### Algorithm Specifications

| Document | Description |
|----------|-------------|
| [xxHash32 Algorithm](./encoding-xxhash32.md) | Complete xxHash32 implementation with JavaScript code, test vectors, and usage examples for Loro checksum verification |
| [LZ4 Frame Format](./encoding-lz4.md) | LZ4 Frame format specification with decompression algorithm and JavaScript implementation |

### Container-Specific Formats

| Document | Description |
|----------|-------------|
| [Container State Snapshots](./encoding-container-states.md) | Detailed encoding formats for Map, List, Text (Richtext), Tree, and MovableList container states |

These documents are essential for implementing a complete Loro encoder/decoder, especially for pure JavaScript implementations without external dependencies.

---

## Checklist for Implementation

To implement a complete Loro decoder/encoder, you need to handle:

- [x] Document header parsing (magic, checksum, mode)
- [x] FastSnapshot body structure
- [x] FastUpdates body structure
- [x] SSTable parsing (header, blocks, meta)
- [x] SSTable Block Chunk internal format (Normal + Large)
- [x] LZ4 decompression for blocks ([see LZ4 spec](./encoding-lz4.md))
- [x] VersionVector encoding/decoding (postcard)
- [x] Frontiers encoding/decoding (postcard)
- [x] ContainerID encoding/decoding
- [x] ContainerWrapper encoding/decoding
- [x] Change Block full parsing
- [x] ContainerArena encoding/decoding (serde_columnar)
- [x] PositionArena encoding/decoding (prefix compression)
- [x] Value encoding/decoding for all types
- [x] serde_columnar compatible decoder
- [x] LEB128 encoder/decoder (unsigned and signed SLEB128)
- [x] BoolRle encoder/decoder
- [x] AnyRle encoder/decoder
- [x] DeltaRle encoder/decoder
- [x] DeltaOfDelta encoder/decoder
- [x] Operation content parsing per container type
- [x] xxHash32 checksum verification ([see xxHash32 spec](./encoding-xxhash32.md))
- [x] Container state snapshot decoding ([see Container States spec](./encoding-container-states.md))
  - [x] MapState snapshot format
  - [x] ListState snapshot format
  - [x] RichtextState (Text) snapshot format
  - [x] TreeState snapshot format
  - [x] MovableListState snapshot format
