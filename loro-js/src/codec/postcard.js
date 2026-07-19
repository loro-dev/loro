import { ByteReader, ByteWriter } from "./bytes";
import { LoroDecodeError, LoroEncodeError } from "./errors";
import { I64_MAX, I64_MIN, U64_MAX, readUleb128, readUlebNumber, writeUleb128, } from "./leb128";
const textDecoder = new TextDecoder("utf-8", { fatal: true });
const textEncoder = new TextEncoder();
// Shared scratch for f64 I/O, avoiding a Uint8Array + DataView allocation
// per call. Never returned to callers.
const f64Scratch = new Uint8Array(8);
const f64View = new DataView(f64Scratch.buffer);
export class PostcardReader {
    input;
    constructor(input) {
        this.input = input instanceof ByteReader ? input : new ByteReader(input);
    }
    readU8() {
        return this.input.readU8();
    }
    readU16() {
        return readUlebNumber(this.input, 0xffff);
    }
    readU32() {
        return readUlebNumber(this.input, 4294967295);
    }
    readU64() {
        return readUleb128(this.input, U64_MAX);
    }
    readUsize() {
        return readUlebNumber(this.input);
    }
    readI32() {
        const value = this.readSigned(32);
        return Number(value);
    }
    readI64() {
        return this.readSigned(64);
    }
    readBool() {
        const value = this.input.readU8();
        if (value === 0) {
            return false;
        }
        if (value === 1) {
            return true;
        }
        throw new LoroDecodeError("invalid postcard boolean", this.input.position - 1);
    }
    readF64() {
        f64Scratch.set(this.input.readBytes(8));
        return f64View.getFloat64(0, true);
    }
    readBytes() {
        return this.input.readBytes(this.readUsize());
    }
    readString() {
        const offset = this.input.position;
        try {
            return textDecoder.decode(this.readBytes());
        }
        catch {
            throw new LoroDecodeError("invalid postcard UTF-8 string", offset);
        }
    }
    readOption(readValue) {
        const tag = this.input.readU8();
        if (tag === 0) {
            return undefined;
        }
        if (tag === 1) {
            return readValue(this);
        }
        throw new LoroDecodeError("invalid postcard option tag", this.input.position - 1);
    }
    readArray(readValue) {
        const length = this.readUsize();
        // push() keeps the array PACKED; new Array(length) + index assignment
        // would produce a permanently holey elements kind.
        const values = [];
        for (let index = 0; index < length; index += 1) {
            values.push(readValue(this, index));
        }
        return values;
    }
    assertEnd() {
        this.input.assertEnd("trailing postcard bytes");
    }
    readSigned(bits) {
        const encoded = readUleb128(this.input, bits === 32 ? 0xffffffffn : U64_MAX);
        const value = (encoded >> 1n) ^ -(encoded & 1n);
        const min = bits === 32 ? -0x80000000n : I64_MIN;
        const max = bits === 32 ? 0x7fffffffn : I64_MAX;
        if (value < min || value > max) {
            throw new LoroDecodeError("postcard signed integer is out of range");
        }
        return value;
    }
}
export class PostcardWriter {
    output;
    constructor(output = new ByteWriter()) {
        this.output = output;
    }
    writeU8(value) {
        this.output.writeU8(value);
    }
    writeU16(value) {
        this.writeUnsignedNumber(value, 0xffff, "u16");
    }
    writeU32(value) {
        this.writeUnsignedNumber(value, 4294967295, "u32");
    }
    writeU64(value) {
        writeUleb128(this.output, value);
    }
    writeUsize(value) {
        this.writeUnsignedNumber(value, Number.MAX_SAFE_INTEGER, "usize");
    }
    writeI32(value) {
        if (!Number.isSafeInteger(value) || value < -2147483648 || value > 2147483647) {
            throw new LoroEncodeError(`i32 is out of range: ${value}`);
        }
        this.writeSigned(BigInt(value), 32);
    }
    writeI64(value) {
        this.writeSigned(value, 64);
    }
    writeBool(value) {
        this.output.writeU8(value ? 1 : 0);
    }
    writeF64(value) {
        f64View.setFloat64(0, value, true);
        this.output.writeBytes(f64Scratch);
    }
    writeBytes(value) {
        this.writeUsize(value.length);
        this.output.writeBytes(value);
    }
    writeString(value) {
        this.writeBytes(textEncoder.encode(value));
    }
    writeOption(value, writeValue) {
        if (value === undefined) {
            this.output.writeU8(0);
            return;
        }
        this.output.writeU8(1);
        writeValue(this, value);
    }
    writeArray(values, writeValue) {
        this.writeUsize(values.length);
        for (const value of values) {
            writeValue(this, value);
        }
    }
    toUint8Array() {
        return this.output.toUint8Array();
    }
    writeUnsignedNumber(value, max, label) {
        if (!Number.isSafeInteger(value) || value < 0 || value > max) {
            throw new LoroEncodeError(`${label} is out of range: ${value}`);
        }
        writeUleb128(this.output, value);
    }
    writeSigned(value, bits) {
        const min = bits === 32 ? -0x80000000n : I64_MIN;
        const max = bits === 32 ? 0x7fffffffn : I64_MAX;
        if (value < min || value > max) {
            throw new LoroEncodeError(`i${bits} is out of range: ${value}`);
        }
        const encoded = (value << 1n) ^ (value >> BigInt(bits - 1));
        writeUleb128(this.output, encoded);
    }
}
