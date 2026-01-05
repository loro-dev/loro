# Loro Export Data Format

This document outlines the binary data format used by `LoroDoc.export`. The format is designed to be compact and efficient for storing and transmitting collaborative editing data.

## Overall File Structure

The exported file consists of a header followed by a body.

### Header

The header contains metadata about the exported data.

| Field       | Size (bytes) | Description                                                                                                                              |
|-------------|--------------|------------------------------------------------------------------------------------------------------------------------------------------|
| Magic Bytes | 4            | A constant value of `b"loro"` (`0x6c`, `0x6f`, `0x72`, `0x6f`) to identify the file as a Loro export.                                       |
| Checksum    | 16           | A checksum to verify the integrity of the data. The first 4 bytes are a [xxhash32](https://github.com/Cyan4973/xxHash) checksum of the body with a seed of `b"LORO"`, and the rest is currently unused. |
| Encode Mode | 2            | An enum indicating the encoding mode used for the body. See [Encode Modes](#encode-modes) for details.                                     |

### Body

The body contains the actual data, which can be either a snapshot or a set of updates. The structure of the body depends on the encode mode.

## Encode Modes

The `EncodeMode` enum determines how the body is structured. The following modes are currently in use:

- `FastSnapshot` (3): A full snapshot of the document's state and history.
- `FastUpdates` (4): A set of updates to the document since a specific version.

## Body Formats

### FastSnapshot Format

The body of a `FastSnapshot` contains three data chunks: the oplog, the state, and the shallow root state (for garbage collection). Each chunk is prefixed by its length.

| Field                        | Size (bytes) | Description                                                                                             |
|------------------------------|--------------|---------------------------------------------------------------------------------------------------------|
| Oplog Bytes Length           | 4            | The length of the oplog bytes, encoded as a little-endian u32.                                          |
| Oplog Bytes                  | variable     | The encoded operations log (oplog), in [SSTable format](#data-chunk-format-sstable).                     |
| State Bytes Length           | 4            | The length of the state bytes, encoded as a little-endian u32.                                          |
| State Bytes                  | variable     | The encoded document state, in [SSTable format](#data-chunk-format-sstable). If empty, this will be a single byte `b"E"` (`0x45`). |
| Shallow Root State Bytes Length | 4            | The length of the shallow root state bytes, encoded as a little-endian u32. |
| Shallow Root State Bytes      | variable     | The encoded shallow root state for garbage collection, in [SSTable format](#data-chunk-format-sstable). |

### FastUpdates Format

The body of a `FastUpdates` consists of a series of data blocks, each representing a chunk of the oplog. Each block is prefixed with its length, encoded as a `leb128` unsigned integer. This allows for streaming updates.

| Field          | Size (bytes) | Description                               |
|----------------|--------------|-------------------------------------------|
| Block 1 Length | 1-5          | The length of Block 1, encoded as leb128. |
| Block 1 Data   | variable     | The data for Block 1 (a chunk of the oplog). |
| Block 2 Length | 1-5          | The length of Block 2, encoded as leb128. |
| Block 2 Data   | variable     | The data for Block 2.                     |
| ...            | ...          | ...                                       |

## Data Chunk Format (SSTable)

The `oplog`, `state`, and `gc` data chunks are stored using a Sorted String Table (SSTable) format. This is a persistent key-value store optimized for write-once, read-many workloads.

### Overall SSTable Structure

| Field         | Size (bytes) | Description                                         |
|---------------|--------------|-----------------------------------------------------|
| Magic Number  | 4            | `b"LORO"`                                           |
| Schema Version| 1            | The version of the SSTable schema.                  |
| Block Chunks  | variable     | A series of data blocks containing key-value pairs. |
| Block Meta    | variable     | Metadata for all blocks.                            |
| Meta Offset   | 4            | The offset of the Block Meta section (little-endian u32). |

### Block Structure

Blocks are optionally compressed using LZ4 and have an `xxhash32` checksum. There are two types of blocks.

#### Normal Block
Stores multiple key-value pairs with prefix compression on keys to save space.

#### Large Value Block
Stores a single key-value pair where the value is large.

For a detailed breakdown of the SSTable block and meta formats, please refer to the documentation within the `crates/kv-store/src/lib.rs` file.

## Source Code References

For more details, you can refer to the following source code files:

- `crates/loro-internal/src/encoding.rs`: Defines the overall encoding structure and `EncodeMode`.
- `crates/loro-internal/src/encoding/fast_snapshot.rs`: Implements the `FastSnapshot` and `FastUpdates` formats.
- `crates/kv-store/src/lib.rs`: Documents and implements the SSTable format used for data chunks.
- `crates/loro-internal/src/loro.rs`: Contains the top-level `export` method.
