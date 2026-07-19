export const LORO_XXHASH_SEED = 1330794316;
const PRIME32_1 = 2654435761;
const PRIME32_2 = 2246822519;
const PRIME32_3 = 3266489917;
const PRIME32_4 = 668265263;
const PRIME32_5 = 374761393;
function rotateLeft(value, count) {
    return ((value << count) | (value >>> (32 - count))) >>> 0;
}
function round(accumulator, input) {
    let result = (accumulator + Math.imul(input, PRIME32_2)) >>> 0;
    result = rotateLeft(result, 13);
    return Math.imul(result, PRIME32_1) >>> 0;
}
function readU32LE(bytes, offset) {
    return ((bytes[offset] |
        (bytes[offset + 1] << 8) |
        (bytes[offset + 2] << 16) |
        (bytes[offset + 3] << 24)) >>>
        0);
}
export function xxhash32(bytes, seed = 0) {
    let offset = 0;
    let hash;
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
    }
    else {
        hash = (seed + PRIME32_5) >>> 0;
    }
    hash = (hash + bytes.length) >>> 0;
    while (offset + 4 <= bytes.length) {
        hash = (hash + Math.imul(readU32LE(bytes, offset), PRIME32_3)) >>> 0;
        hash = Math.imul(rotateLeft(hash, 17), PRIME32_4) >>> 0;
        offset += 4;
    }
    while (offset < bytes.length) {
        hash = (hash + Math.imul(bytes[offset], PRIME32_5)) >>> 0;
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
