import { ByteReader, ByteWriter } from "./bytes";
import { LoroDecodeError, LoroEncodeError, decodeAssert } from "./errors";
import { I64_MAX, I64_MIN, U64_MAX, readUlebNumber, writeUleb128 } from "./leb128";
import { PostcardReader, PostcardWriter } from "./postcard";

const U32_MAX = 0xffff_ffff;
const I32_MIN = -0x8000_0000;
const I32_MAX = 0x7fff_ffff;
const I128_MIN = -(1n << 127n);
const I128_MAX = (1n << 127n) - 1n;
const U128_MAX = (1n << 128n) - 1n;
const MAX_COLUMN_VALUES = 10_000_000;

export function decodeColumnarVec(bytes: Uint8Array): Uint8Array[] {
  const reader = new ByteReader(bytes);
  const columns = readColumnarVec(reader);
  reader.assertEnd("trailing serde-columnar bytes");
  return columns;
}

export function takeColumnarVec(bytes: Uint8Array): [Uint8Array[], Uint8Array] {
  const reader = new ByteReader(bytes);
  const columns = readColumnarVec(reader);
  return [columns, bytes.subarray(reader.position)];
}

export function decodeColumnarVecMaybeWrapped(bytes: Uint8Array): Uint8Array[] {
  if (bytes.length === 0) {
    return [];
  }
  const reader = new ByteReader(bytes);
  if (readUlebNumber(reader, MAX_COLUMN_VALUES) === 1) {
    const columns = readColumnarVec(reader);
    reader.assertEnd("trailing wrapped serde-columnar bytes");
    return columns;
  }
  return decodeColumnarVec(bytes);
}

export function encodeColumnarVec(columns: readonly Uint8Array[]): Uint8Array {
  const writer = new ByteWriter();
  writeUleb128(writer, columns.length);
  for (const column of columns) {
    writeUleb128(writer, column.length);
    writer.writeBytes(column);
  }
  return writer.toUint8Array();
}

export function encodeColumnarVecWrapped(columns: readonly Uint8Array[]): Uint8Array {
  const writer = new ByteWriter();
  writeUleb128(writer, 1);
  writer.writeBytes(encodeColumnarVec(columns));
  return writer.toUint8Array();
}

export function decodeBoolRle(bytes: Uint8Array): boolean[] {
  const reader = new ByteReader(bytes);
  const values: boolean[] = [];
  let state = false;
  while (reader.remaining > 0) {
    const length = readUlebNumber(reader, MAX_COLUMN_VALUES);
    assertDecodedLength(values.length, length, "bool RLE");
    for (let index = 0; index < length; index += 1) {
      values.push(state);
    }
    state = !state;
  }
  return values;
}

export function encodeBoolRle(values: readonly boolean[]): Uint8Array {
  const writer = new ByteWriter();
  if (values.length === 0) {
    return writer.toUint8Array();
  }
  let state = false;
  let runLength = 0;
  for (const value of values) {
    if (value === state) {
      runLength += 1;
    } else {
      writeUleb128(writer, runLength);
      state = !state;
      runLength = 1;
    }
  }
  writeUleb128(writer, runLength);
  return writer.toUint8Array();
}

export function takeBoolRle(bytes: Uint8Array, count: number): [boolean[], Uint8Array] {
  assertCount(count);
  const reader = new ByteReader(bytes);
  const values: boolean[] = [];
  let state = false;
  while (values.length < count) {
    decodeAssert(reader.remaining > 0, "bool RLE has too few elements", reader.position);
    const length = readUlebNumber(reader, MAX_COLUMN_VALUES);
    decodeAssert(values.length + length <= count, "bool RLE has too many elements");
    for (let index = 0; index < length; index += 1) {
      values.push(state);
    }
    state = !state;
  }
  return [values, bytes.subarray(reader.position)];
}

export function decodeRleU8(bytes: Uint8Array): number[] {
  return decodeAnyRle(bytes, readPostcardU8);
}

export function encodeRleU8(values: readonly number[]): Uint8Array {
  return encodeAnyRleLiteral(values, writePostcardU8);
}

export function decodeRleU32(bytes: Uint8Array): number[] {
  return decodeAnyRle(bytes, readPostcardU32);
}

export function encodeRleU32(values: readonly number[]): Uint8Array {
  return encodeAnyRleLiteral(values, writePostcardU32);
}

export function decodeAnyRleU32(bytes: Uint8Array): number[] {
  return decodeAnyRle(bytes, readPostcardU32);
}

export function encodeAnyRleU32(values: readonly number[]): Uint8Array {
  return encodeAnyRleLiteral(values, writePostcardU32);
}

export function takeAnyRleU32(bytes: Uint8Array, count: number): [number[], Uint8Array] {
  return takeAnyRle(bytes, count, readPostcardU32);
}

export function decodeAnyRleU64(bytes: Uint8Array): bigint[] {
  return decodeAnyRle(bytes, readPostcardU64);
}

export function encodeAnyRleU64(values: readonly bigint[]): Uint8Array {
  return encodeAnyRleLiteral(values, writePostcardU64);
}

export function decodeAnyRleUsize(bytes: Uint8Array): bigint[] {
  return decodeAnyRleU64(bytes);
}

export function encodeAnyRleUsize(values: readonly bigint[]): Uint8Array {
  return encodeAnyRleU64(values);
}

export function takeAnyRleUsize(
  bytes: Uint8Array,
  count: number,
): [bigint[], Uint8Array] {
  return takeAnyRle(bytes, count, readPostcardU64);
}

export function decodeAnyRleI32(bytes: Uint8Array): number[] {
  return decodeAnyRle(bytes, readPostcardI32);
}

export function encodeAnyRleI32(values: readonly number[]): Uint8Array {
  return encodeAnyRleLiteral(values, writePostcardI32);
}

export function takeAnyRleI32(bytes: Uint8Array, count: number): [number[], Uint8Array] {
  return takeAnyRle(bytes, count, readPostcardI32);
}

export function decodeDeltaRleU32(bytes: Uint8Array): number[] {
  return decodeDeltaNumber(bytes, 0, U32_MAX);
}

export function encodeDeltaRleU32(values: readonly number[]): Uint8Array {
  return encodeDeltaNumber(values, assertU32);
}

export function decodeDeltaRleI32(bytes: Uint8Array): number[] {
  return decodeDeltaNumber(bytes, I32_MIN, I32_MAX);
}

export function encodeDeltaRleI32(values: readonly number[]): Uint8Array {
  return encodeDeltaNumber(values, assertI32);
}

export function decodeDeltaRleUsize(bytes: Uint8Array): bigint[] {
  return decodeDelta(bytes, 0n, U64_MAX, identity);
}

export function encodeDeltaRleUsize(values: readonly bigint[]): Uint8Array {
  return encodeDelta(values, assertUsize);
}

export function decodeDeltaRleIsize(bytes: Uint8Array): bigint[] {
  return decodeDelta(bytes, I64_MIN, I64_MAX, identity);
}

export function encodeDeltaRleIsize(values: readonly bigint[]): Uint8Array {
  return encodeDelta(values, assertIsize);
}

export function decodeDeltaOfDeltaI64(bytes: Uint8Array): bigint[] {
  const reader = new ByteReader(bytes);
  const first = readPostcardOptionalI64(reader);
  decodeAssert(reader.remaining >= 1, "invalid delta-of-delta bytes", reader.position);
  const lastUsedBits = reader.readU8();
  const bitstream = reader.readRemaining();
  if (first === undefined) {
    decodeAssert(
      lastUsedBits === 0 && bitstream.length === 0,
      "invalid empty delta-of-delta encoding",
    );
    return [];
  }
  const bits = BitReader.forCompleteStream(bitstream, lastUsedBits);
  const values = [first];
  let previous = first;
  let delta = 0n;
  while (bits.remaining > 0) {
    delta = checkedI64(delta + decodeDeltaOfDeltaValue(bits), "delta-of-delta delta");
    previous = checkedI64(previous + delta, "delta-of-delta value");
    values.push(previous);
  }
  return values;
}

export function takeDeltaOfDeltaI64(
  bytes: Uint8Array,
  count: number,
): [bigint[], Uint8Array] {
  assertCount(count);
  const reader = new ByteReader(bytes);
  const first = readPostcardOptionalI64(reader);
  decodeAssert(reader.remaining >= 1, "invalid delta-of-delta bytes", reader.position);
  const lastUsedBits = reader.readU8();
  const bitstreamOffset = reader.position;
  if (first === undefined) {
    decodeAssert(count === 0, "delta-of-delta has too few elements");
    decodeAssert(lastUsedBits === 0, "invalid empty delta-of-delta encoding");
    return [[], bytes.subarray(bitstreamOffset)];
  }
  decodeAssert(count > 0, "delta-of-delta has too many elements");
  const bits = BitReader.forPrefix(bytes.subarray(bitstreamOffset));
  const values = [first];
  let previous = first;
  let delta = 0n;
  while (values.length < count) {
    delta = checkedI64(delta + decodeDeltaOfDeltaValue(bits), "delta-of-delta delta");
    previous = checkedI64(previous + delta, "delta-of-delta value");
    values.push(previous);
  }
  if (count === 1) {
    decodeAssert(lastUsedBits === 0, "invalid single-value delta-of-delta encoding");
  } else {
    const expectedLastUsed = bits.position % 8 || 8;
    decodeAssert(
      lastUsedBits === expectedLastUsed,
      "delta-of-delta last-used-bits mismatch",
    );
  }
  return [values, bytes.subarray(bitstreamOffset + Math.ceil(bits.position / 8))];
}

export function encodeDeltaOfDeltaI64(values: readonly bigint[]): Uint8Array {
  const writer = new ByteWriter();
  if (values.length === 0) {
    writePostcardOptionalI64(writer, undefined);
    writer.writeU8(0);
    return writer.toUint8Array();
  }
  for (const value of values) {
    assertBigIntRange(value, I64_MIN, I64_MAX, "i64");
  }
  writePostcardOptionalI64(writer, values[0]!);
  if (values.length === 1) {
    writer.writeU8(0);
    return writer.toUint8Array();
  }
  const bits = new BitWriter();
  let previousDelta = 0n;
  for (let index = 1; index < values.length; index += 1) {
    const delta = checkedI64(values[index]! - values[index - 1]!, "delta-of-delta delta");
    const deltaOfDelta = checkedI64(delta - previousDelta, "delta-of-delta difference");
    previousDelta = delta;
    encodeDeltaOfDeltaValue(bits, deltaOfDelta);
  }
  const encoded = bits.finish();
  writer.writeU8(encoded.lastUsedBits);
  writer.writeBytes(encoded.bytes);
  return writer.toUint8Array();
}

function readColumnarVec(reader: ByteReader): Uint8Array[] {
  const count = readUlebNumber(reader, MAX_COLUMN_VALUES);
  const columns: Uint8Array[] = [];
  for (let index = 0; index < count; index += 1) {
    columns.push(reader.readBytes(readUlebNumber(reader, 0x7fff_ffff)));
  }
  return columns;
}

function decodeAnyRle<T>(bytes: Uint8Array, readValue: (reader: ByteReader) => T): T[] {
  const reader = new ByteReader(bytes);
  const values: T[] = [];
  while (reader.remaining > 0) {
    readAnyRleSegment(reader, values, MAX_COLUMN_VALUES, readValue);
  }
  return values;
}

function takeAnyRle<T>(
  bytes: Uint8Array,
  count: number,
  readValue: (reader: ByteReader) => T,
): [T[], Uint8Array] {
  assertCount(count);
  const reader = new ByteReader(bytes);
  const values: T[] = [];
  while (values.length < count) {
    decodeAssert(reader.remaining > 0, "AnyRle has too few elements", reader.position);
    readAnyRleSegment(reader, values, count, readValue);
  }
  return [values, bytes.subarray(reader.position)];
}

function readAnyRleSegment<T>(
  reader: ByteReader,
  values: T[],
  limit: number,
  readValue: (reader: ByteReader) => T,
): void {
  const signedLength = readPostcardI64(reader);
  decodeAssert(signedLength !== 0n, "AnyRle segment has zero length", reader.position);
  const lengthBigInt = signedLength < 0n ? -signedLength : signedLength;
  decodeAssert(lengthBigInt <= BigInt(MAX_COLUMN_VALUES), "AnyRle segment is too long");
  const length = Number(lengthBigInt);
  decodeAssert(values.length + length <= limit, "AnyRle has too many elements");
  if (signedLength > 0n) {
    const value = readValue(reader);
    for (let index = 0; index < length; index += 1) {
      values.push(value);
    }
    return;
  }
  for (let index = 0; index < length; index += 1) {
    values.push(readValue(reader));
  }
}

function encodeAnyRleLiteral<T>(
  values: readonly T[],
  writeValue: (writer: ByteWriter, value: T) => void,
): Uint8Array {
  const writer = new ByteWriter();
  if (values.length === 0) {
    return writer.toUint8Array();
  }
  if (values.length > MAX_COLUMN_VALUES) {
    throw new LoroEncodeError(`AnyRle input is too long: ${values.length}`);
  }
  writePostcardI64(writer, -BigInt(values.length));
  for (const value of values) {
    writeValue(writer, value);
  }
  return writer.toUint8Array();
}

function encodeDelta<T>(
  values: readonly T[],
  toBigInt: (value: T) => bigint,
): Uint8Array {
  const writer = new ByteWriter();
  if (values.length === 0) {
    return writer.toUint8Array();
  }
  if (values.length > MAX_COLUMN_VALUES) {
    throw new LoroEncodeError(`AnyRle input is too long: ${values.length}`);
  }
  writePostcardI64(writer, -BigInt(values.length));
  let previous = 0n;
  for (const input of values) {
    const value = toBigInt(input);
    const delta = value - previous;
    assertBigIntRange(delta, I128_MIN, I128_MAX, "delta RLE delta");
    writePostcardI128(writer, delta);
    previous = value;
  }
  return writer.toUint8Array();
}

function decodeDelta<T>(
  bytes: Uint8Array,
  min: bigint,
  max: bigint,
  fromBigInt: (value: bigint) => T,
): T[] {
  const reader = new ByteReader(bytes);
  const values: T[] = [];
  let value = 0n;
  const append = (delta: bigint): void => {
    value += delta;
    decodeAssert(value >= min && value <= max, "delta RLE value is out of range");
    values.push(fromBigInt(value));
  };
  while (reader.remaining > 0) {
    const signedLength = readPostcardI64(reader);
    decodeAssert(signedLength !== 0n, "AnyRle segment has zero length", reader.position);
    const lengthBigInt = signedLength < 0n ? -signedLength : signedLength;
    decodeAssert(lengthBigInt <= BigInt(MAX_COLUMN_VALUES), "AnyRle segment is too long");
    const length = Number(lengthBigInt);
    decodeAssert(
      values.length + length <= MAX_COLUMN_VALUES,
      "AnyRle has too many elements",
    );
    if (signedLength > 0n) {
      const delta = readPostcardI128(reader);
      for (let index = 0; index < length; index += 1) {
        append(delta);
      }
    } else {
      for (let index = 0; index < length; index += 1) {
        append(readPostcardI128(reader));
      }
    }
  }
  return values;
}

function encodeDeltaNumber(
  values: readonly number[],
  validate: (value: number) => number,
): Uint8Array {
  const writer = new ByteWriter();
  if (values.length === 0) {
    return writer.toUint8Array();
  }
  if (values.length > MAX_COLUMN_VALUES) {
    throw new LoroEncodeError(`AnyRle input is too long: ${values.length}`);
  }
  writePostcardI64(writer, -BigInt(values.length));
  let previous = 0;
  for (const input of values) {
    const value = validate(input);
    writePostcardI128Number(writer, value - previous);
    previous = value;
  }
  return writer.toUint8Array();
}

function decodeDeltaNumber(bytes: Uint8Array, min: number, max: number): number[] {
  const reader = new ByteReader(bytes);
  const values: number[] = [];
  let value = 0;
  const append = (delta: number): void => {
    value += delta;
    decodeAssert(value >= min && value <= max, "delta RLE value is out of range");
    values.push(value);
  };
  while (reader.remaining > 0) {
    const signedLength = readPostcardI64(reader);
    decodeAssert(signedLength !== 0n, "AnyRle segment has zero length", reader.position);
    const lengthBigInt = signedLength < 0n ? -signedLength : signedLength;
    decodeAssert(lengthBigInt <= BigInt(MAX_COLUMN_VALUES), "AnyRle segment is too long");
    const length = Number(lengthBigInt);
    decodeAssert(
      values.length + length <= MAX_COLUMN_VALUES,
      "AnyRle has too many elements",
    );
    if (signedLength > 0n) {
      const delta = readPostcardI128Number(reader);
      for (let index = 0; index < length; index += 1) {
        append(delta);
      }
    } else {
      for (let index = 0; index < length; index += 1) {
        append(readPostcardI128Number(reader));
      }
    }
  }
  return values;
}

function readPostcardU8(reader: ByteReader): number {
  return reader.readU8();
}

function writePostcardU8(writer: ByteWriter, value: number): void {
  writer.writeU8(value);
}

function readPostcardU32(reader: ByteReader): number {
  return new PostcardReader(reader).readU32();
}

function writePostcardU32(writer: ByteWriter, value: number): void {
  new PostcardWriter(writer).writeU32(value);
}

function readPostcardU64(reader: ByteReader): bigint {
  return new PostcardReader(reader).readU64();
}

function writePostcardU64(writer: ByteWriter, value: bigint): void {
  new PostcardWriter(writer).writeU64(value);
}

function readPostcardI32(reader: ByteReader): number {
  return new PostcardReader(reader).readI32();
}

function writePostcardI32(writer: ByteWriter, value: number): void {
  new PostcardWriter(writer).writeI32(value);
}

function readPostcardI64(reader: ByteReader): bigint {
  return new PostcardReader(reader).readI64();
}

function writePostcardI64(writer: ByteWriter, value: bigint): void {
  new PostcardWriter(writer).writeI64(value);
}

function readPostcardOptionalI64(reader: ByteReader): bigint | undefined {
  const tag = readPostcardU64(reader);
  if (tag === 0n) {
    return undefined;
  }
  if (tag === 1n) {
    return readPostcardI64(reader);
  }
  throw new LoroDecodeError("invalid postcard Option<i64> tag", reader.position);
}

function writePostcardOptionalI64(writer: ByteWriter, value: bigint | undefined): void {
  writePostcardU64(writer, value === undefined ? 0n : 1n);
  if (value !== undefined) {
    writePostcardI64(writer, value);
  }
}

function readPostcardI128(reader: ByteReader): bigint {
  const encoded = readVarintU128(reader);
  const value = (encoded >> 1n) ^ -(encoded & 1n);
  decodeAssert(value >= I128_MIN && value <= I128_MAX, "postcard i128 is out of range");
  return value;
}

function writePostcardI128(writer: ByteWriter, value: bigint): void {
  assertBigIntRange(value, I128_MIN, I128_MAX, "i128");
  const encoded = value >= 0n ? value << 1n : (-value << 1n) - 1n;
  writeVarintU128(writer, encoded);
}

function readPostcardI128Number(reader: ByteReader): number {
  const start = reader.position;
  let encoded = 0;
  let multiplier = 1;
  for (let index = 0; index < 19; index += 1) {
    const byte = reader.readU8();
    const payload = byte & 0x7f;
    if (payload !== 0 && multiplier > Number.MAX_SAFE_INTEGER / payload) {
      throw new LoroDecodeError("i128 varint exceeds safe integer range", start);
    }
    encoded += payload * multiplier;
    if (!Number.isSafeInteger(encoded)) {
      throw new LoroDecodeError("i128 varint exceeds safe integer range", start);
    }
    if ((byte & 0x80) === 0) {
      return encoded % 2 === 0 ? encoded / 2 : -(encoded + 1) / 2;
    }
    multiplier *= 128;
  }
  throw new LoroDecodeError("u128 varint overflow", start);
}

function writePostcardI128Number(writer: ByteWriter, value: number): void {
  const encoded = value >= 0 ? value * 2 : -value * 2 - 1;
  let remaining = encoded;
  do {
    let byte = remaining % 128;
    remaining = Math.floor(remaining / 128);
    if (remaining !== 0) {
      byte |= 0x80;
    }
    writer.writeU8(byte);
  } while (remaining !== 0);
}

function readVarintU128(reader: ByteReader): bigint {
  const start = reader.position;
  let value = 0n;
  for (let index = 0; index < 19; index += 1) {
    const byte = reader.readU8();
    const payload = BigInt(byte & 0x7f);
    if (index === 18 && payload > 3n) {
      throw new LoroDecodeError("u128 varint overflow", start);
    }
    value |= payload << BigInt(index * 7);
    if ((byte & 0x80) === 0) {
      return value;
    }
  }
  throw new LoroDecodeError("u128 varint overflow", start);
}

function writeVarintU128(writer: ByteWriter, input: bigint): void {
  assertBigIntRange(input, 0n, U128_MAX, "u128");
  let value = input;
  do {
    let byte = Number(value & 0x7fn);
    value >>= 7n;
    if (value !== 0n) {
      byte |= 0x80;
    }
    writer.writeU8(byte);
  } while (value !== 0n);
}

class BitReader {
  readonly #bytes: Uint8Array;
  readonly #totalBits: number;
  #position = 0;

  private constructor(bytes: Uint8Array, totalBits: number) {
    this.#bytes = bytes;
    this.#totalBits = totalBits;
  }

  static forCompleteStream(bytes: Uint8Array, lastUsedBits: number): BitReader {
    decodeAssert(lastUsedBits >= 0 && lastUsedBits <= 8, "invalid last-used-bits value");
    if (lastUsedBits === 0) {
      decodeAssert(bytes.length === 0, "unexpected delta-of-delta bitstream bytes");
      return new BitReader(bytes, 0);
    }
    decodeAssert(bytes.length > 0, "missing delta-of-delta bitstream bytes");
    return new BitReader(bytes, (bytes.length - 1) * 8 + lastUsedBits);
  }

  static forPrefix(bytes: Uint8Array): BitReader {
    return new BitReader(bytes, bytes.length * 8);
  }

  get position(): number {
    return this.#position;
  }

  get remaining(): number {
    return this.#totalBits - this.#position;
  }

  readBits(count: number): bigint {
    decodeAssert(
      Number.isSafeInteger(count) && count >= 0 && count <= 64,
      "invalid bit width",
    );
    decodeAssert(this.remaining >= count, "unexpected end of delta-of-delta bitstream");
    let value = 0n;
    for (let index = 0; index < count; index += 1) {
      const byte = this.#bytes[Math.floor(this.#position / 8)]!;
      const shift = 7 - (this.#position % 8);
      value = (value << 1n) | BigInt((byte >>> shift) & 1);
      this.#position += 1;
    }
    return value;
  }
}

class BitWriter {
  readonly #bytes: number[] = [];
  #current = 0;
  #used = 0;

  writeBits(value: bigint, count: number): void {
    for (let shift = count - 1; shift >= 0; shift -= 1) {
      this.#current = (this.#current << 1) | Number((value >> BigInt(shift)) & 1n);
      this.#used += 1;
      if (this.#used === 8) {
        this.#bytes.push(this.#current);
        this.#current = 0;
        this.#used = 0;
      }
    }
  }

  finish(): { readonly bytes: Uint8Array; readonly lastUsedBits: number } {
    if (this.#used === 0) {
      return { bytes: Uint8Array.from(this.#bytes), lastUsedBits: 8 };
    }
    this.#bytes.push(this.#current << (8 - this.#used));
    return { bytes: Uint8Array.from(this.#bytes), lastUsedBits: this.#used };
  }
}

function decodeDeltaOfDeltaValue(reader: BitReader): bigint {
  if (reader.readBits(1) === 0n) {
    return 0n;
  }
  if (reader.readBits(1) === 0n) {
    return reader.readBits(7) - 63n;
  }
  if (reader.readBits(1) === 0n) {
    return reader.readBits(9) - 255n;
  }
  if (reader.readBits(1) === 0n) {
    return reader.readBits(12) - 2047n;
  }
  if (reader.readBits(1) === 0n) {
    return reader.readBits(21) - 1_048_575n;
  }
  return BigInt.asIntN(64, reader.readBits(64));
}

function encodeDeltaOfDeltaValue(writer: BitWriter, value: bigint): void {
  if (value === 0n) {
    writer.writeBits(0n, 1);
  } else if (value >= -63n && value <= 64n) {
    writer.writeBits(0b10n, 2);
    writer.writeBits(value + 63n, 7);
  } else if (value >= -255n && value <= 256n) {
    writer.writeBits(0b110n, 3);
    writer.writeBits(value + 255n, 9);
  } else if (value >= -2047n && value <= 2048n) {
    writer.writeBits(0b1110n, 4);
    writer.writeBits(value + 2047n, 12);
  } else if (value >= -1_048_575n && value <= 1_048_576n) {
    writer.writeBits(0b11110n, 5);
    writer.writeBits(value + 1_048_575n, 21);
  } else {
    writer.writeBits(0b11111n, 5);
    writer.writeBits(BigInt.asUintN(64, value), 64);
  }
}

function checkedI64(value: bigint, label: string): bigint {
  decodeAssert(value >= I64_MIN && value <= I64_MAX, `${label} is out of range`);
  return value;
}

function assertDecodedLength(current: number, extra: number, label: string): void {
  decodeAssert(current + extra <= MAX_COLUMN_VALUES, `${label} output is too long`);
}

function assertCount(count: number): void {
  if (!Number.isSafeInteger(count) || count < 0 || count > MAX_COLUMN_VALUES) {
    throw new LoroDecodeError(`invalid requested column value count: ${count}`);
  }
}

function assertU32(value: number): number {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xffff_ffff) {
    throw new LoroEncodeError(`u32 is out of range: ${value}`);
  }
  return value;
}

function assertI32(value: number): number {
  if (!Number.isSafeInteger(value) || value < -0x8000_0000 || value > 0x7fff_ffff) {
    throw new LoroEncodeError(`i32 is out of range: ${value}`);
  }
  return value;
}

function assertUsize(value: bigint): bigint {
  assertBigIntRange(value, 0n, U64_MAX, "usize");
  return value;
}

function assertIsize(value: bigint): bigint {
  assertBigIntRange(value, I64_MIN, I64_MAX, "isize");
  return value;
}

function identity<T>(value: T): T {
  return value;
}

function assertBigIntRange(value: bigint, min: bigint, max: bigint, label: string): void {
  if (value < min || value > max) {
    throw new LoroEncodeError(`${label} is out of range: ${value}`);
  }
}
