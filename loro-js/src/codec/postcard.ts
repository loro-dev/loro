import { ByteReader, ByteWriter } from "./bytes";
import { LoroDecodeError, LoroEncodeError } from "./errors";
import {
  I64_MAX,
  I64_MIN,
  U64_MAX,
  readUleb128,
  readUlebNumber,
  writeUleb128,
} from "./leb128";

const textDecoder = new TextDecoder("utf-8", { fatal: true });
const textEncoder = new TextEncoder();
const MIN_FAST_I64 = -4_503_599_627_370_496n;
const MAX_FAST_I64 = 4_503_599_627_370_495n;

export class PostcardReader {
  readonly input: ByteReader;

  constructor(input: ByteReader | Uint8Array) {
    this.input = input instanceof ByteReader ? input : new ByteReader(input);
  }

  readU8(): number {
    return this.input.readU8();
  }

  readU16(): number {
    return readUlebNumber(this.input, 0xffff);
  }

  readU32(): number {
    return readUlebNumber(this.input, 0xffff_ffff);
  }

  readU64(): bigint {
    return readUleb128(this.input, U64_MAX);
  }

  readUsize(): number {
    return readUlebNumber(this.input);
  }

  readI32(): number {
    const value = this.readSigned(32);
    return Number(value);
  }

  readI64(): bigint {
    return this.readSigned(64);
  }

  readBool(): boolean {
    const value = this.input.readU8();
    if (value === 0) {
      return false;
    }
    if (value === 1) {
      return true;
    }
    throw new LoroDecodeError("invalid postcard boolean", this.input.position - 1);
  }

  readF64(): number {
    const bytes = this.input.readBytes(8);
    return new DataView(bytes.buffer, bytes.byteOffset, 8).getFloat64(0, true);
  }

  readBytes(): Uint8Array {
    return this.input.readBytes(this.readUsize());
  }

  readString(): string {
    const offset = this.input.position;
    try {
      return textDecoder.decode(this.readBytes());
    } catch {
      throw new LoroDecodeError("invalid postcard UTF-8 string", offset);
    }
  }

  readOption<T>(readValue: (reader: PostcardReader) => T): T | undefined {
    const tag = this.input.readU8();
    if (tag === 0) {
      return undefined;
    }
    if (tag === 1) {
      return readValue(this);
    }
    throw new LoroDecodeError("invalid postcard option tag", this.input.position - 1);
  }

  readArray<T>(readValue: (reader: PostcardReader, index: number) => T): T[] {
    const length = this.readUsize();
    const values = new Array<T>(length);
    for (let index = 0; index < length; index += 1) {
      values[index] = readValue(this, index);
    }
    return values;
  }

  assertEnd(): void {
    this.input.assertEnd("trailing postcard bytes");
  }

  private readSigned(bits: 32 | 64): bigint {
    const encoded = readUleb128(this.input, bits === 32 ? 0xffff_ffffn : U64_MAX);
    const value = (encoded >> 1n) ^ -(encoded & 1n);
    const min = bits === 32 ? -0x8000_0000n : I64_MIN;
    const max = bits === 32 ? 0x7fff_ffffn : I64_MAX;
    if (value < min || value > max) {
      throw new LoroDecodeError("postcard signed integer is out of range");
    }
    return value;
  }
}

export class PostcardWriter {
  readonly output: ByteWriter;

  constructor(output = new ByteWriter()) {
    this.output = output;
  }

  writeU8(value: number): void {
    this.output.writeU8(value);
  }

  writeU16(value: number): void {
    this.writeUnsignedNumber(value, 0xffff, "u16");
  }

  writeU32(value: number): void {
    this.writeUnsignedNumber(value, 0xffff_ffff, "u32");
  }

  writeU64(value: bigint): void {
    writeUleb128(this.output, value);
  }

  writeUsize(value: number): void {
    this.writeUnsignedNumber(value, Number.MAX_SAFE_INTEGER, "usize");
  }

  writeI32(value: number): void {
    if (!Number.isSafeInteger(value) || value < -0x8000_0000 || value > 0x7fff_ffff) {
      throw new LoroEncodeError(`i32 is out of range: ${value}`);
    }
    writeUleb128(this.output, value >= 0 ? value * 2 : -value * 2 - 1);
  }

  writeI64(value: bigint): void {
    if (value >= MIN_FAST_I64 && value <= MAX_FAST_I64) {
      const number = Number(value);
      writeUleb128(this.output, number >= 0 ? number * 2 : -number * 2 - 1);
      return;
    }
    this.writeSigned(value, 64);
  }

  writeBool(value: boolean): void {
    this.output.writeU8(value ? 1 : 0);
  }

  writeF64(value: number): void {
    const bytes = new Uint8Array(8);
    new DataView(bytes.buffer).setFloat64(0, value, true);
    this.output.writeBytes(bytes);
  }

  writeBytes(value: Uint8Array): void {
    this.writeUsize(value.length);
    this.output.writeBytes(value);
  }

  writeString(value: string): void {
    this.writeBytes(textEncoder.encode(value));
  }

  writeOption<T>(
    value: T | undefined,
    writeValue: (writer: PostcardWriter, value: T) => void,
  ): void {
    if (value === undefined) {
      this.output.writeU8(0);
      return;
    }
    this.output.writeU8(1);
    writeValue(this, value);
  }

  writeArray<T>(
    values: readonly T[],
    writeValue: (writer: PostcardWriter, value: T) => void,
  ): void {
    this.writeUsize(values.length);
    for (const value of values) {
      writeValue(this, value);
    }
  }

  toUint8Array(): Uint8Array {
    return this.output.toUint8Array();
  }

  private writeUnsignedNumber(value: number, max: number, label: string): void {
    if (!Number.isSafeInteger(value) || value < 0 || value > max) {
      throw new LoroEncodeError(`${label} is out of range: ${value}`);
    }
    writeUleb128(this.output, value);
  }

  private writeSigned(value: bigint, bits: 32 | 64): void {
    const min = bits === 32 ? -0x8000_0000n : I64_MIN;
    const max = bits === 32 ? 0x7fff_ffffn : I64_MAX;
    if (value < min || value > max) {
      throw new LoroEncodeError(`i${bits} is out of range: ${value}`);
    }
    const encoded = (value << 1n) ^ (value >> BigInt(bits - 1));
    writeUleb128(this.output, encoded);
  }
}
