//! # MemKvStore Documentation
//!
//! MemKvStore use SSTable as backend. The SSTable (Sorted String Table) is a persistent data structure
//! used for storing key-value pairs in a sorted manner. This document describes the binary format of
//! the SSTable.
//!
//! ## Overall Structure
//!
//! The SSTable consists of the following sections:
//!
//! ┌─────────────────────────────────────────────────────────────────────────────────────────────────┐
//! │ MemKVStore                                                                                      │
//! │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ┐│
//! │  Magic Number │ Schema Version │ Block Chunk   ...  │  Block Chunk    Block Meta │ Meta Offset  │
//! ││     u32      │       u8       │    bytes    │      │     bytes     │   bytes    │     u32     ││
//! │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ │
//! └─────────────────────────────────────────────────────────────────────────────────────────────────┘
//!
//! 1. Magic Number (4 bytes): A fixed value "LORO" to identify the file format.
//! 2. Schema Version (1 byte): The version of the MemKVStore schema.
//! 3. Block Chunks: A series of data blocks containing key-value pairs.
//! 4. Block Meta: Metadata for all blocks, including block offset, the first key of the block, `is_large` flag, and last key
//!    if not large.
//! 5. Meta Offset (4 bytes): The offset of the Block Meta section from the beginning of the file.
//!
//! ## Block Types
//!
//! There are two types of blocks: Normal Blocks and Large Value Blocks.
//!
//! ### Normal Block
//!
//! Normal blocks store multiple key-value pairs with compressed keys.
//!
//! ┌────────────────────────────────────────────────────────────────────────────────────────────┐
//! │Block                                                                                       │
//! │┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─┌ ─ ─ ─┌ ─ ─ ─ ┬ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─     │
//! │ Key Value Chunk  ...  │Key Value Chunk  offset │ ...  │ offset  kv len │Block Checksum│    │
//! ││     bytes     │      │     bytes     │  u16   │      │  u16  │  u16   │     u32           │
//! │ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ┘    │
//! └────────────────────────────────────────────────────────────────────────────────────────────┘
//!
//! Each Key Value Chunk is encoded as follows:
//!
//! ┌─────────────────────────────────────────────────────┐
//! │  Key Value Chunk                                    │
//! │┌ ─ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─┬ ─ ─ ─ ┐│
//! │ common prefix len key suffix len│key suffix│ value ││
//! ││       u8        │     u16      │  bytes   │ bytes ││
//! │ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ┘─ ─ ─ ─┘│
//! └─────────────────────────────────────────────────────┘
//!
//! Encoding:
//! 1. Compress key-value pairs data as Key Value Chunk.
//! 2. Write offsets for each key-value pair.
//! 3. Write the number of key-value pairs.
//! 4. **Compress** the entire block using LZ4.
//! 5. Calculate and append xxhash_32 checksum.
//!
//! Decoding:
//! 1. Verify the xxhash_32 checksum.
//! 2. **Decompress** the block using LZ4.
//! 3. Read the number of key-value pairs.
//! 4. Read offsets for each key-value pair.
//! 5. Parse individual key-value chunks.
//!
//! ### Large Value Block
//!
//! Large Value Blocks store a single key-value pair with a large value.
//!
//! ┌──────────────────────────┐
//! │Large Block               │
//! │┌ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ─ ─ │
//! │  value   Block Checksum ││
//! ││ bytes │      u32        │
//! │ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘│
//! └──────────────────────────┘
//!
//! Encoding:
//! 1. Write the value bytes.
//! 2. Calculate and append xxhash_32 checksum.
//!
//! Decoding:
//! 1. Verify the xxhash_32 checksum.
//! 2. Read the value bytes.
//!
//! We need not encode the length of value, because we can get the whole Block by offset in meta.
//!
//! ## Block Meta
//!
//! The Block Meta section contains metadata for all blocks in the SSTable.
//!
//! ┌────────────────────────────────────────────────────────────┐
//! │ All Block Meta                                             │
//! │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─┌ ─ ─ ─┌ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ │
//! │  block length │ Block Meta │ ...  │ Block Meta │ checksum ││
//! ││     u32      │   bytes    │      │   bytes    │   u32     │
//! │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ┘─ ─ ─ ┘─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ┘│
//! └────────────────────────────────────────────────────────────┘
//!
//! Each Block Meta entry is encoded as follows:
//!
//! ┌──────────────────────────────────────────────────────────────────────────────────────┐
//! │ Block Meta                                                                           │
//! │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ┐ │
//! │  block offset │ first key len   first key   is large │ last key len     last key     │
//! ││     u32      │      u16      │   bytes   │    u8    │  u16(option)  │bytes(option)│ │
//! │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  │
//! └──────────────────────────────────────────────────────────────────────────────────────┘
//!
//! Encoding:
//! 1. Write the number of blocks.
//! 2. For each block, write its metadata (offset, first key, is_large flag, and last key if not large).
//! 3. Calculate and append xxhash_32 checksum.
//!
//! Decoding:
//! 1. Read the number of blocks.
//! 2. For each block, read its metadata.
//! 3. Verify the xxhash_32 checksum.
//!
pub mod iter;
pub mod mem_store;
pub mod sstable;
pub use mem_store::MemKvStore;
