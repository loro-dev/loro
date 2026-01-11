# LZ4 Frame Format Specification

This document describes the LZ4 Frame format used by Loro for SSTable block compression. Loro uses the standard LZ4 Frame format as defined by the LZ4 specification.

## Overview

Loro uses LZ4 compression for SSTable blocks when it reduces size. The compression uses **LZ4 Frame format** (not raw LZ4 block format), which includes:
- Magic number for format identification
- Frame descriptor with compression settings
- One or more data blocks
- Optional content checksum

**Note**: For pure JavaScript implementations, it's recommended to use an existing LZ4 library (like `lz4js`) rather than implementing from scratch. This document is provided for completeness and for those who need to understand the format.

## LZ4 Frame Structure

```
┌────────────────────────────────────────────────────────────────────────────┐
│                           LZ4 Frame Format                                  │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ 4 bytes         │ Magic Number: 0x184D2204 (little-endian)                 │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 3-15 bytes      │ Frame Descriptor                                         │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ variable        │ Data Block(s)                                            │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 4 bytes         │ End Mark: 0x00000000                                     │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 0 or 4 bytes    │ Content Checksum (optional, xxHash32)                    │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

## Frame Descriptor

```
┌────────────────────────────────────────────────────────────────────────────┐
│                         Frame Descriptor                                    │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ 1 byte          │ FLG (Flags)                                              │
│                 │   Bit 7-6: Version (must be 01)                          │
│                 │   Bit 5: Block Independence (1=independent, 0=linked)    │
│                 │   Bit 4: Block Checksum flag (1=present)                 │
│                 │   Bit 3: Content Size flag (1=present)                   │
│                 │   Bit 2: Content Checksum flag (1=present)               │
│                 │   Bit 1: Reserved (must be 0)                            │
│                 │   Bit 0: DictID flag (1=present)                         │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 1 byte          │ BD (Block Descriptor)                                    │
│                 │   Bit 7: Reserved (must be 0)                            │
│                 │   Bit 6-4: Block Max Size                                │
│                 │     4 = 64 KB, 5 = 256 KB, 6 = 1 MB, 7 = 4 MB            │
│                 │   Bit 3-0: Reserved (must be 0)                          │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 0 or 8 bytes    │ Content Size (if flag set, little-endian u64)            │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 0 or 4 bytes    │ Dictionary ID (if flag set, little-endian u32)           │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 1 byte          │ Header Checksum (xxHash32 of descriptor >> 8 & 0xFF)     │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

## Data Block Format

Each data block has the following structure:

```
┌────────────────────────────────────────────────────────────────────────────┐
│                           Data Block                                        │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ 4 bytes         │ Block Size (little-endian)                               │
│                 │   Bit 31: Uncompressed flag (1=uncompressed)             │
│                 │   Bit 30-0: Compressed size in bytes                     │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ variable        │ Block Data (compressed or uncompressed)                  │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 0 or 4 bytes    │ Block Checksum (if Block Checksum flag set, xxHash32)    │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

## LZ4 Block Compression Algorithm

### Sequence Format

LZ4 compressed data consists of sequences. Each sequence has:

```
┌────────────────────────────────────────────────────────────────────────────┐
│                           LZ4 Sequence                                      │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ 1 byte          │ Token                                                    │
│                 │   High nibble (bits 7-4): Literal length (0-15)          │
│                 │   Low nibble (bits 3-0): Match length - 4 (0-15)         │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 0+ bytes        │ Extended Literal Length (if high nibble = 15)            │
│                 │   Add consecutive 255s, then final byte < 255            │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ variable        │ Literals (literal_length bytes)                          │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 2 bytes         │ Match Offset (little-endian, 1-65535)                    │
│                 │   (Not present in last sequence if no match)             │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 0+ bytes        │ Extended Match Length (if low nibble = 15)               │
│                 │   Add consecutive 255s, then final byte < 255            │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

### Decompression Algorithm

```javascript
function decompressLZ4Block(input) {
  const output = [];
  let inputPos = 0;

  while (inputPos < input.length) {
    // Read token
    const token = input[inputPos++];
    let literalLength = token >> 4;
    let matchLength = (token & 0x0F) + 4;

    // Extended literal length
    if (literalLength === 15) {
      let byte;
      do {
        byte = input[inputPos++];
        literalLength += byte;
      } while (byte === 255);
    }

    // Copy literals
    for (let i = 0; i < literalLength; i++) {
      output.push(input[inputPos++]);
    }

    // Check if this is the last sequence (no match after literals)
    if (inputPos >= input.length) {
      break;
    }

    // Read match offset (little-endian)
    const offset = input[inputPos] | (input[inputPos + 1] << 8);
    inputPos += 2;

    // Extended match length
    if ((token & 0x0F) === 15) {
      let byte;
      do {
        byte = input[inputPos++];
        matchLength += byte;
      } while (byte === 255);
    }

    // Copy match (can overlap with output)
    const matchPos = output.length - offset;
    for (let i = 0; i < matchLength; i++) {
      output.push(output[matchPos + i]);
    }
  }

  return new Uint8Array(output);
}
```

### Frame Decompression

```javascript
const LZ4_MAGIC = 0x184D2204;

function decompressLZ4Frame(input) {
  let pos = 0;
  const view = new DataView(input.buffer, input.byteOffset);

  // Check magic number
  const magic = view.getUint32(pos, true);
  if (magic !== LZ4_MAGIC) {
    throw new Error('Invalid LZ4 magic number');
  }
  pos += 4;

  // Parse frame descriptor
  const flg = input[pos++];
  const bd = input[pos++];

  const version = (flg >> 6) & 0x03;
  if (version !== 1) {
    throw new Error('Unsupported LZ4 version');
  }

  const blockIndependence = (flg >> 5) & 0x01;
  const blockChecksum = (flg >> 4) & 0x01;
  const contentSizeFlag = (flg >> 3) & 0x01;
  const contentChecksum = (flg >> 2) & 0x01;
  const dictIdFlag = flg & 0x01;

  const blockMaxSize = (bd >> 4) & 0x07;

  // Skip optional content size
  if (contentSizeFlag) {
    pos += 8;
  }

  // Skip optional dict ID
  if (dictIdFlag) {
    pos += 4;
  }

  // Skip header checksum
  pos += 1;

  // Decompress blocks
  const output = [];

  while (pos < input.length) {
    // Read block size
    const blockSize = view.getUint32(pos, true);
    pos += 4;

    // End mark
    if (blockSize === 0) {
      break;
    }

    const isUncompressed = (blockSize >> 31) & 0x01;
    const size = blockSize & 0x7FFFFFFF;

    const blockData = input.slice(pos, pos + size);
    pos += size;

    // Skip block checksum if present
    if (blockChecksum) {
      pos += 4;
    }

    if (isUncompressed) {
      // Uncompressed block
      for (let i = 0; i < blockData.length; i++) {
        output.push(blockData[i]);
      }
    } else {
      // Compressed block
      const decompressed = decompressLZ4Block(blockData);
      for (let i = 0; i < decompressed.length; i++) {
        output.push(decompressed[i]);
      }
    }
  }

  // Skip content checksum if present
  if (contentChecksum && pos + 4 <= input.length) {
    // Optionally verify checksum here using xxHash32
    pos += 4;
  }

  return new Uint8Array(output);
}
```

## Loro-Specific Usage

### Compression Detection

In Loro's SSTable format, the compression type is stored in the block meta flags:

```javascript
function getCompressionType(flags) {
  const compressionBits = flags & 0x7F;  // Lower 7 bits
  switch (compressionBits) {
    case 0: return 'none';
    case 1: return 'lz4';
    default: throw new Error('Unknown compression type');
  }
}
```

### Block Decompression

```javascript
function decompressLoroBlock(blockBytes, compressionType) {
  if (compressionType === 'none') {
    return blockBytes;
  } else if (compressionType === 'lz4') {
    return decompressLZ4Frame(blockBytes);
  }
  throw new Error('Unknown compression type');
}
```

## Compression (Encoding)

For encoding, the compression algorithm is more complex. Here's a simplified version:

```javascript
function compressLZ4Block(input) {
  // This is a simplified encoder - production code should use
  // hash tables for match finding

  const output = [];
  let inputPos = 0;
  let anchorPos = 0;

  // Minimum match length is 4
  const MIN_MATCH = 4;

  while (inputPos < input.length) {
    let matchLength = 0;
    let matchOffset = 0;

    // Find best match using hash table (simplified: linear search)
    for (let distance = 1; distance <= Math.min(inputPos, 65535); distance++) {
      let len = 0;
      while (
        inputPos + len < input.length &&
        input[inputPos - distance + len] === input[inputPos + len]
      ) {
        len++;
      }
      if (len >= MIN_MATCH && len > matchLength) {
        matchLength = len;
        matchOffset = distance;
      }
    }

    if (matchLength < MIN_MATCH) {
      inputPos++;
      continue;
    }

    // Emit sequence
    const literalLength = inputPos - anchorPos;

    // Token
    let token = 0;
    token |= Math.min(literalLength, 15) << 4;
    token |= Math.min(matchLength - 4, 15);
    output.push(token);

    // Extended literal length
    if (literalLength >= 15) {
      let remaining = literalLength - 15;
      while (remaining >= 255) {
        output.push(255);
        remaining -= 255;
      }
      output.push(remaining);
    }

    // Literals
    for (let i = anchorPos; i < inputPos; i++) {
      output.push(input[i]);
    }

    // Match offset (little-endian)
    output.push(matchOffset & 0xFF);
    output.push((matchOffset >> 8) & 0xFF);

    // Extended match length
    if (matchLength - 4 >= 15) {
      let remaining = matchLength - 4 - 15;
      while (remaining >= 255) {
        output.push(255);
        remaining -= 255;
      }
      output.push(remaining);
    }

    inputPos += matchLength;
    anchorPos = inputPos;
  }

  // Emit final literals
  if (anchorPos < input.length) {
    const literalLength = input.length - anchorPos;

    let token = Math.min(literalLength, 15) << 4;
    output.push(token);

    if (literalLength >= 15) {
      let remaining = literalLength - 15;
      while (remaining >= 255) {
        output.push(255);
        remaining -= 255;
      }
      output.push(remaining);
    }

    for (let i = anchorPos; i < input.length; i++) {
      output.push(input[i]);
    }
  }

  return new Uint8Array(output);
}
```

## Test Vectors

```javascript
// Simple test case
const input = new Uint8Array([
  0x48, 0x65, 0x6C, 0x6C, 0x6F, 0x2C, 0x20,  // "Hello, "
  0x57, 0x6F, 0x72, 0x6C, 0x64, 0x21         // "World!"
]);

// When no compression benefit, data stored uncompressed
// Compressed with LZ4 Frame format:
const compressed = new Uint8Array([
  0x04, 0x22, 0x4D, 0x18,  // Magic number (LE)
  0x60, 0x40,              // FLG, BD (block independent, 64KB max)
  0x82,                    // Header checksum
  0x0D, 0x00, 0x00, 0x80,  // Block size (13 bytes, uncompressed flag set)
  // ... original data ...
  0x00, 0x00, 0x00, 0x00   // End mark
]);
```

## Recommendations

1. **Use an existing library**: LZ4 compression is complex. Use a well-tested library like:
   - JavaScript: `lz4js`, `lz4-wasm`
   - Node.js: `lz4`

2. **Decompression only**: For read-only Loro implementations, only decompression is needed

3. **Fallback handling**: Loro falls back to uncompressed if compression increases size

## Reference

- LZ4 Frame Format: https://github.com/lz4/lz4/blob/dev/doc/lz4_Frame_format.md
- LZ4 Block Format: https://github.com/lz4/lz4/blob/dev/doc/lz4_Block_format.md

---

**Source**: Based on LZ4 specification and Loro's usage at `crates/kv-store/src/compress.rs`
