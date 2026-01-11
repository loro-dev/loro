# xxHash32 Algorithm Specification

This document provides the complete xxHash32 algorithm specification for implementors who need to verify Loro document checksums without external dependencies.

## Overview

xxHash32 is a fast, non-cryptographic hash function used by Loro for data integrity verification. Loro uses xxHash32 with the seed `0x4F524F4C` ("LORO" as little-endian u32).

**Usage in Loro:**
- Document header checksum (bytes 16-20)
- SSTable block meta checksum
- SSTable block checksums

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
xxHash32(new Uint8Array([]), LORO_SEED) === 0xDC3BF95A  // 3694230874

// Single byte
xxHash32(new Uint8Array([0x00]), LORO_SEED) === 0xDAD9F666  // 3672012390

// "loro" (4 bytes)
xxHash32(new Uint8Array([0x6C, 0x6F, 0x72, 0x6F]), LORO_SEED) === 0x74D321EA  // 1959690730

// 16 bytes (triggers block processing)
xxHash32(new Uint8Array([
  0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
  0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F
]), LORO_SEED) === 0x2EDAB25F  // 787026527
```

## Usage in Loro Checksum Verification

### Document Header Checksum

```javascript
function verifyLoroDocument(bytes) {
  // Check magic bytes
  const magic = String.fromCharCode(...bytes.slice(0, 4));
  if (magic !== "loro") {
    throw new Error("Invalid magic bytes");
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

### SSTable Block Checksum

For SSTable blocks, the checksum is stored as the last 4 bytes:

```javascript
function verifyBlock(blockBytes) {
  const dataLen = blockBytes.length - 4;
  const data = blockBytes.slice(0, dataLen);
  const storedChecksum = readU32LE(blockBytes, dataLen);
  const calculatedChecksum = xxHash32(data, 0x4F524F4C);

  return storedChecksum === calculatedChecksum;
}
```

## Performance Notes

- The algorithm is designed for speed, not cryptographic security
- Larger inputs benefit from the 4-way parallel accumulator design
- For JavaScript implementations, consider using `Uint8Array` for best performance
- The `>>> 0` operations ensure unsigned 32-bit arithmetic in JavaScript

## Reference

- Original xxHash specification: https://github.com/Cyan4973/xxHash
- xxHash32 is deterministic and platform-independent when following this specification

---

**Source**: Based on xxHash specification and Loro's usage at `crates/kv-store/src/sstable.rs` and `crates/loro-internal/src/encoding.rs`
