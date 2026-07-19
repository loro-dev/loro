import { LoroDecodeError, LoroEncodeError, decodeAssert, encodeAssert } from "./errors";
const U8_MAX = 0xff;
const U16_MAX = 0xffff;
const U32_MAX = 4294967295;
const U64_MAX = 0xffffffffffffffffn;
export class ByteReader {
    bytes;
    #offset;
    #end;
    constructor(bytes, offset = 0, length = bytes.length - offset) {
        decodeAssert(Number.isSafeInteger(offset) && offset >= 0, "invalid reader offset");
        decodeAssert(Number.isSafeInteger(length) && length >= 0, "invalid reader length");
        decodeAssert(offset + length <= bytes.length, "reader range is out of bounds");
        this.bytes = bytes;
        this.#offset = offset;
        this.#end = offset + length;
    }
    get position() {
        return this.#offset;
    }
    get remaining() {
        return this.#end - this.#offset;
    }
    readU8() {
        this.require(1);
        const value = this.bytes[this.#offset];
        this.#offset += 1;
        return value;
    }
    readU16LE() {
        const a = this.readU8();
        const b = this.readU8();
        return a | (b << 8);
    }
    readU16BE() {
        const a = this.readU8();
        const b = this.readU8();
        return (a << 8) | b;
    }
    readU32LE() {
        const a = this.readU8();
        const b = this.readU8();
        const c = this.readU8();
        const d = this.readU8();
        return (a | (b << 8) | (c << 16) | (d << 24)) >>> 0;
    }
    readU32BE() {
        const a = this.readU8();
        const b = this.readU8();
        const c = this.readU8();
        const d = this.readU8();
        return ((a << 24) | (b << 16) | (c << 8) | d) >>> 0;
    }
    readU64LE() {
        let value = 0n;
        for (let shift = 0n; shift < 64n; shift += 8n) {
            value |= BigInt(this.readU8()) << shift;
        }
        return value;
    }
    readU64BE() {
        let value = 0n;
        for (let i = 0; i < 8; i += 1) {
            value = (value << 8n) | BigInt(this.readU8());
        }
        return value;
    }
    readBytes(length) {
        this.require(length);
        const start = this.#offset;
        this.#offset += length;
        return this.bytes.subarray(start, this.#offset);
    }
    readRemaining() {
        return this.readBytes(this.remaining);
    }
    skip(length) {
        this.require(length);
        this.#offset += length;
    }
    assertEnd(message = "trailing bytes") {
        if (this.remaining !== 0) {
            throw new LoroDecodeError(message, this.#offset);
        }
    }
    require(length) {
        if (!Number.isSafeInteger(length) || length < 0) {
            throw new LoroDecodeError("invalid read length", this.#offset);
        }
        if (length > this.remaining) {
            throw new LoroDecodeError("unexpected end of input", this.#offset);
        }
    }
}
export class ByteWriter {
    #buffer;
    #length = 0;
    constructor(initialCapacity = 64) {
        encodeAssert(Number.isSafeInteger(initialCapacity) && initialCapacity >= 0, "invalid writer capacity");
        this.#buffer = new Uint8Array(initialCapacity);
    }
    get length() {
        return this.#length;
    }
    writeU8(value) {
        assertUnsignedNumber(value, U8_MAX, "u8");
        this.ensureCapacity(1);
        this.#buffer[this.#length] = value;
        this.#length += 1;
    }
    writeU16LE(value) {
        assertUnsignedNumber(value, U16_MAX, "u16");
        this.writeU8(value & U8_MAX);
        this.writeU8(value >>> 8);
    }
    writeU16BE(value) {
        assertUnsignedNumber(value, U16_MAX, "u16");
        this.writeU8(value >>> 8);
        this.writeU8(value & U8_MAX);
    }
    writeU32LE(value) {
        assertUnsignedNumber(value, U32_MAX, "u32");
        this.writeU8(value & U8_MAX);
        this.writeU8((value >>> 8) & U8_MAX);
        this.writeU8((value >>> 16) & U8_MAX);
        this.writeU8((value >>> 24) & U8_MAX);
    }
    writeU32BE(value) {
        assertUnsignedNumber(value, U32_MAX, "u32");
        this.writeU8((value >>> 24) & U8_MAX);
        this.writeU8((value >>> 16) & U8_MAX);
        this.writeU8((value >>> 8) & U8_MAX);
        this.writeU8(value & U8_MAX);
    }
    writeU64LE(value) {
        assertUnsignedBigInt(value, U64_MAX, "u64");
        for (let shift = 0n; shift < 64n; shift += 8n) {
            this.writeU8(Number((value >> shift) & 0xffn));
        }
    }
    writeU64BE(value) {
        assertUnsignedBigInt(value, U64_MAX, "u64");
        for (let shift = 56n; shift >= 0n; shift -= 8n) {
            this.writeU8(Number((value >> shift) & 0xffn));
        }
    }
    writeBytes(bytes) {
        this.ensureCapacity(bytes.length);
        this.#buffer.set(bytes, this.#length);
        this.#length += bytes.length;
    }
    toUint8Array() {
        // When the buffer is exactly full, hand it over instead of copying; any
        // later write would reallocate, so the returned array stays intact. This
        // matches Lz4Output.finish in lz4.ts.
        return this.#length === this.#buffer.length
            ? this.#buffer
            : this.#buffer.slice(0, this.#length);
    }
    ensureCapacity(extra) {
        const required = this.#length + extra;
        if (required <= this.#buffer.length) {
            return;
        }
        let capacity = Math.max(64, this.#buffer.length);
        while (capacity < required) {
            capacity = Math.max(required, capacity * 2);
        }
        const next = new Uint8Array(capacity);
        next.set(this.#buffer.subarray(0, this.#length));
        this.#buffer = next;
    }
}
function assertUnsignedNumber(value, max, label) {
    if (!Number.isSafeInteger(value) || value < 0 || value > max) {
        throw new LoroEncodeError(`${label} is out of range: ${value}`);
    }
}
function assertUnsignedBigInt(value, max, label) {
    if (value < 0n || value > max) {
        throw new LoroEncodeError(`${label} is out of range: ${value}`);
    }
}
export function concatBytes(...parts) {
    let length = 0;
    for (const part of parts) {
        length += part.length;
        encodeAssert(Number.isSafeInteger(length), "byte sequence is too large");
    }
    const output = new Uint8Array(length);
    let offset = 0;
    for (const part of parts) {
        output.set(part, offset);
        offset += part.length;
    }
    return output;
}
export function bytesEqual(a, b) {
    if (a.length !== b.length) {
        return false;
    }
    for (let i = 0; i < a.length; i += 1) {
        if (a[i] !== b[i]) {
            return false;
        }
    }
    return true;
}
export function compareBytes(a, b) {
    const length = Math.min(a.length, b.length);
    for (let i = 0; i < length; i += 1) {
        const diff = a[i] - b[i];
        if (diff !== 0) {
            return diff;
        }
    }
    return a.length - b.length;
}
export function bytesToHex(bytes) {
    let output = "";
    for (const byte of bytes) {
        output += byte.toString(16).padStart(2, "0");
    }
    return output;
}
export function hexToBytes(hex) {
    if (hex.length % 2 !== 0 || !/^[0-9a-f]*$/iu.test(hex)) {
        throw new LoroDecodeError("invalid hexadecimal byte string");
    }
    const output = new Uint8Array(hex.length / 2);
    for (let i = 0; i < output.length; i += 1) {
        output[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
    }
    return output;
}
