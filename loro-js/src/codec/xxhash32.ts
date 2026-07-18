export const LORO_XXHASH_SEED = 0x4f52_4f4c;

const PRIME32_1 = 0x9e37_79b1;
const PRIME32_2 = 0x85eb_ca77;
const PRIME32_3 = 0xc2b2_ae3d;
const PRIME32_4 = 0x27d4_eb2f;
const PRIME32_5 = 0x1656_67b1;

function rotateLeft(value: number, count: number): number {
  return ((value << count) | (value >>> (32 - count))) >>> 0;
}

function round(accumulator: number, input: number): number {
  let result = (accumulator + Math.imul(input, PRIME32_2)) >>> 0;
  result = rotateLeft(result, 13);
  return Math.imul(result, PRIME32_1) >>> 0;
}

function readU32LE(bytes: Uint8Array, offset: number): number {
  return (
    (bytes[offset]! |
      (bytes[offset + 1]! << 8) |
      (bytes[offset + 2]! << 16) |
      (bytes[offset + 3]! << 24)) >>>
    0
  );
}

export function xxhash32(bytes: Uint8Array, seed = 0): number {
  let offset = 0;
  let hash: number;

  if (bytes.length >= 16) {
    let v1 = (seed + PRIME32_1 + PRIME32_2) >>> 0;
    let v2 = (seed + PRIME32_2) >>> 0;
    let v3 = seed >>> 0;
    let v4 = (seed - PRIME32_1) >>> 0;
    const limit = bytes.length - 16;
    while (offset <= limit) {
      v1 = round(v1, readU32LE(bytes, offset));
      offset += 4;
      v2 = round(v2, readU32LE(bytes, offset));
      offset += 4;
      v3 = round(v3, readU32LE(bytes, offset));
      offset += 4;
      v4 = round(v4, readU32LE(bytes, offset));
      offset += 4;
    }
    hash =
      (rotateLeft(v1, 1) +
        rotateLeft(v2, 7) +
        rotateLeft(v3, 12) +
        rotateLeft(v4, 18)) >>>
      0;
  } else {
    hash = (seed + PRIME32_5) >>> 0;
  }

  hash = (hash + bytes.length) >>> 0;
  while (offset + 4 <= bytes.length) {
    hash = (hash + Math.imul(readU32LE(bytes, offset), PRIME32_3)) >>> 0;
    hash = Math.imul(rotateLeft(hash, 17), PRIME32_4) >>> 0;
    offset += 4;
  }
  while (offset < bytes.length) {
    hash = (hash + Math.imul(bytes[offset]!, PRIME32_5)) >>> 0;
    hash = Math.imul(rotateLeft(hash, 11), PRIME32_1) >>> 0;
    offset += 1;
  }

  hash ^= hash >>> 15;
  hash = Math.imul(hash, PRIME32_2) >>> 0;
  hash ^= hash >>> 13;
  hash = Math.imul(hash, PRIME32_3) >>> 0;
  hash ^= hash >>> 16;
  return hash >>> 0;
}
