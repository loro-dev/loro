# xxHash32 in current Loro encodings

Verified against code 2026-07-17 at commit
`fd5a1fdab79142302f0c0fbceb8807128ec6d9cd` and locked dependency
`xxhash-rust 0.8.15`.

This document provides the complete xxHash32 algorithm for implementations that
verify current Loro document and SSTable checksums without an external hash
library.

## Overview

xxHash32 is a non-cryptographic integrity hash. Loro's document envelope,
SSTable metadata, and SSTable block checksums use seed `0x4f524f4c`, obtained as
`u32::from_le_bytes(*b"LORO")`.

LZ4 frame-internal xxHash checks use seed zero and are a separate domain. See
[encoding-lz4.md](./encoding-lz4.md). Neither form is a cryptographic
authenticator.

Current Loro-seeded usage:

- mode-3/mode-4 document checksum at bytes 16..20;
- SSTable block-metadata checksum; and
- every normal or large SSTable block checksum.

Dependency pin: [`Cargo.lock`](../Cargo.lock#L3738-L3744). The published crate
archive's `.cargo_vcs_info.json` pins source revision
`7026cd705195f502283f97aafc9ea41930099c68`.

## Algorithm Specification

### Constants

```javascript
const PRIME32_1 = 0x9E3779B1;  // 2654435761
const PRIME32_2 = 0x85EBCA77;  // 2246822519
const PRIME32_3 = 0xC2B2AE3D;  // 3266489917
const PRIME32_4 = 0x27D4EB2F;  // 668265263
const PRIME32_5 = 0x165667B1;  // 374761393
```

### Helper Functions

```javascript
// Rotate left for 32-bit integers
function rotl32(x, r) {
  return ((x << r) | (x >>> (32 - r))) >>> 0;
}

// Read 32-bit little-endian integer
function readU32LE(bytes, offset) {
  return (
    bytes[offset] |
    (bytes[offset + 1] << 8) |
    (bytes[offset + 2] << 16) |
    (bytes[offset + 3] << 24)
  ) >>> 0;
}

// 32-bit multiplication (handles JavaScript number overflow)
function mul32(a, b) {
  const aLow = a & 0xFFFF;
  const aHigh = a >>> 16;
  const bLow = b & 0xFFFF;
  const bHigh = b >>> 16;

  const low = aLow * bLow;
  const mid = (aLow * bHigh + aHigh * bLow) & 0xFFFF;

  return ((low + (mid << 16)) & 0xFFFFFFFF) >>> 0;
}

// Round function
function round(acc, input) {
  acc = (acc + mul32(input, PRIME32_2)) >>> 0;
  acc = rotl32(acc, 13);
  acc = mul32(acc, PRIME32_1);
  return acc;
}
```

### Main Algorithm

```javascript
function xxHash32(data, seed = 0) {
  const len = data.length;
  let h32;
  let offset = 0;

  if (len >= 16) {
    // Initialize accumulators
    let v1 = (seed + PRIME32_1 + PRIME32_2) >>> 0;
    let v2 = (seed + PRIME32_2) >>> 0;
    let v3 = seed >>> 0;
    let v4 = (seed - PRIME32_1) >>> 0;

    // Process 16-byte blocks
    const limit = len - 16;
    while (offset <= limit) {
      v1 = round(v1, readU32LE(data, offset)); offset += 4;
      v2 = round(v2, readU32LE(data, offset)); offset += 4;
      v3 = round(v3, readU32LE(data, offset)); offset += 4;
      v4 = round(v4, readU32LE(data, offset)); offset += 4;
    }

    // Merge accumulators
    h32 = (rotl32(v1, 1) + rotl32(v2, 7) + rotl32(v3, 12) + rotl32(v4, 18)) >>> 0;
  } else {
    // Small input: use seed + PRIME32_5
    h32 = (seed + PRIME32_5) >>> 0;
  }

  // Add length
  h32 = (h32 + len) >>> 0;

  // Process remaining 4-byte chunks
  while (offset + 4 <= len) {
    h32 = (h32 + mul32(readU32LE(data, offset), PRIME32_3)) >>> 0;
    h32 = mul32(rotl32(h32, 17), PRIME32_4);
    offset += 4;
  }

  // Process remaining bytes
  while (offset < len) {
    h32 = (h32 + mul32(data[offset], PRIME32_5)) >>> 0;
    h32 = mul32(rotl32(h32, 11), PRIME32_1);
    offset += 1;
  }

  // Final avalanche
  h32 ^= h32 >>> 15;
  h32 = mul32(h32, PRIME32_2);
  h32 ^= h32 >>> 13;
  h32 = mul32(h32, PRIME32_3);
  h32 ^= h32 >>> 16;

  return h32 >>> 0;
}
```

## Test Vectors

Use these test vectors to verify your implementation:

```javascript
// Empty input
xxHash32(new Uint8Array([]), 0) === 0x02CC5D05  // 46947589

// "LORO" seed (used by Loro)
const LORO_SEED = 0x4F524F4C;

// Empty with LORO seed
xxHash32(new Uint8Array([]), LORO_SEED) === 0xDC3BF95A  // 3694917978

// Single byte
xxHash32(new Uint8Array([0x00]), LORO_SEED) === 0xDAD9F666  // 3671717478

// "loro" (4 bytes)
xxHash32(new Uint8Array([0x6C, 0x6F, 0x72, 0x6F]), LORO_SEED) === 0x74D321EA  // 1959993834

// 16 bytes (triggers block processing)
xxHash32(new Uint8Array([
  0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
  0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F
]), LORO_SEED) === 0x2EDAB25F  // 786084447
```

## Usage in Loro checksum verification

All stored Loro checksums below are fixed-width `u32le`, even though many
neighboring fields use varints.

### Document envelope checksum

For current modes, the document must contain at least 22 bytes, have lowercase
magic `loro`, and have big-endian mode 3 or 4. The stored checksum is bytes
16..20. The hash input begins at byte 20, so it includes the two mode bytes and
the complete body:

```javascript
function verifyLoroDocument(bytes) {
  if (bytes.length < 22) {
    throw new Error("Document is shorter than the 22-byte header");
  }

  // Check magic bytes
  const magic = String.fromCharCode(...bytes.slice(0, 4));
  if (magic !== "loro") {
    throw new Error("Invalid magic bytes");
  }

  // Current mode is u16 big-endian and must be FastSnapshot or FastUpdates.
  const mode = (bytes[20] << 8) | bytes[21];
  if (mode !== 3 && mode !== 4) {
    throw new Error("Unsupported current encoding mode");
  }

  // Read stored checksum (bytes 16-20, little-endian)
  const storedChecksum = readU32LE(bytes, 16);

  // Calculate checksum of encode_mode + body (bytes 20+)
  const payload = bytes.slice(20);
  const calculatedChecksum = xxHash32(payload, 0x4F524F4C);

  if (storedChecksum !== calculatedChecksum) {
    throw new Error("Checksum mismatch");
  }

  return true;
}
```

The other 12 bytes in the 16-byte checksum field are written as zero by the
current encoder and ignored by the current mode-3/mode-4 checksum reader.

Writer/reader:
[`encoding.rs::encode_with`](../crates/loro-internal/src/encoding.rs#L440-L459),
[`ParsedHeaderAndBody::check_checksum`, `parse_header_and_body`](../crates/loro-internal/src/encoding.rs#L300-L384).

### SSTable metadata checksum

The metadata region has `u32le block_count`, zero or more metadata entries,
then a final checksum. The hash excludes both the count and the checksum:

```javascript
function verifySSTableMeta(metaBytes) {
  if (metaBytes.length < 8) {
    throw new Error("Truncated SSTable metadata");
  }

  const checksumOffset = metaBytes.length - 4;
  const stored = readU32LE(metaBytes, checksumOffset);
  const entries = metaBytes.slice(4, checksumOffset);
  return stored === xxHash32(entries, 0x4F524F4C);
}
```

This covers every encoded entry byte beginning with the first block offset. It
does not cover the leading block count, the final checksum itself, or the
SSTable's trailing `meta_offset` field.

Writer/reader:
[`BlockMeta::encode_meta`, `decode_meta`](../crates/kv-store/src/sstable.rs#L49-L148).

### SSTable block checksum

Every normal or large SSTable block ends in a checksum over the stored body.
When compression type is LZ4, “stored body” means the complete LZ4 frame, not
the decompressed bytes:

```javascript
function verifyBlock(blockBytes) {
  if (blockBytes.length < 4) {
    throw new Error("Truncated SSTable block");
  }

  const dataLen = blockBytes.length - 4;
  const data = blockBytes.slice(0, dataLen);
  const storedChecksum = readU32LE(blockBytes, dataLen);
  const calculatedChecksum = xxHash32(data, 0x4F524F4C);

  return storedChecksum === calculatedChecksum;
}
```

Normal/large writers:
[`block.rs`](../crates/kv-store/src/block.rs#L25-L116).
Reader:
[`SSTable::check_block_checksum`](../crates/kv-store/src/sstable.rs#L489-L518).

## Domain summary

| Domain | Stored form | Seed | Hash input |
|---|---|---:|---|
| current document | bytes 16..20, `u32le` | `0x4f524f4c` | document bytes 20..EOF |
| SSTable metadata | last 4 metadata bytes, `u32le` | `0x4f524f4c` | metadata entries only; exclude count and checksum |
| SSTable block | last 4 block bytes, `u32le` | `0x4f524f4c` | stored block body before checksum |
| LZ4 header HC | one byte | 0 | LZ4 descriptor, then take hash bits 8..15 |

Optional LZ4 block/content checksum domains also use seed zero and are not
emitted by the current Loro writer. They are specified in the LZ4 document.

## Reference and implementation notes

- The JavaScript `mul32` helper deliberately returns the low 32 bits of the
  product. `Math.imul(a, b) >>> 0` is an equivalent implementation.
- The `>>> 0` conversions keep JavaScript arithmetic in the intended unsigned
  32-bit domain.
- Original algorithm: [xxHash](https://github.com/Cyan4973/xxHash).
- Locked Rust implementation: `xxhash-rust 0.8.15`, feature `xxh32`.

The test vectors above were checked against the locked Rust implementation.
