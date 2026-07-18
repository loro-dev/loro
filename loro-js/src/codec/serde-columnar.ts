import { ByteReader, ByteWriter } from "./bytes";
import { LoroDecodeError, LoroEncodeError, decodeAssert, encodeAssert } from "./errors";
import { I64_MAX, I64_MIN, U64_MAX, readUleb128, readUlebNumber, writeUleb128 } from "./leb128";

const U32_MAX = 0xffff_ffff;
const I32_MIN = -0x8000_0000;
const I32_MAX = 0x7fff_ffff;
const I128_MIN = -(1n << 127n);
const I128_MAX = (1n << 127n) - 1n;
const U128_MAX = (1n << 128n) - 1n;
const MAX_COLUMN_VALUES = 10_000_000;
const MIN_SAFE_BIGINT = BigInt(-Number.MAX_SAFE_INTEGER);
const MAX_SAFE_BIGINT = BigInt(Number.MAX_SAFE_INTEGER);

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
  const values: bigint[] = [first];
  let previous = first;
  let delta = 0n;
  // Number fast path: while every intermediate value stays a safe integer,
  // accumulate with plain number arithmetic and box only the emitted element.
  let previousNumber = 0;
  let deltaNumber = 0;
  let useNumber = first >= MIN_SAFE_BIGINT && first <= MAX_SAFE_BIGINT;
  if (useNumber) {
    previousNumber = Number(first);
  }
  while (bits.remaining > 0) {
    const step = decodeDeltaOfDeltaValue(bits);
    if (useNumber) {
      if (typeof step === "number") {
        const nextDelta = deltaNumber + step;
        const next = previousNumber + nextDelta;
        if (Number.isSafeInteger(nextDelta) && Number.isSafeInteger(next)) {
          deltaNumber = nextDelta;
          previousNumber = next;
          values.push(BigInt(next));
          continue;
        }
      }
      delta = BigInt(deltaNumber);
      previous = BigInt(previousNumber);
      useNumber = false;
    }
    const stepBigInt = typeof step === "number" ? BigInt(step) : step;
    delta = checkedI64(delta + stepBigInt, "delta-of-delta delta");
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
  const values: bigint[] = [first];
  let previous = first;
  let delta = 0n;
  // Number fast path, mirroring decodeDeltaOfDeltaI64.
  let previousNumber = 0;
  let deltaNumber = 0;
  let useNumber = first >= MIN_SAFE_BIGINT && first <= MAX_SAFE_BIGINT;
  if (useNumber) {
    previousNumber = Number(first);
  }
  while (values.length < count) {
    const step = decodeDeltaOfDeltaValue(bits);
    if (useNumber) {
      if (typeof step === "number") {
        const nextDelta = deltaNumber + step;
        const next = previousNumber + nextDelta;
        if (Number.isSafeInteger(nextDelta) && Number.isSafeInteger(next)) {
          deltaNumber = nextDelta;
          previousNumber = next;
          values.push(BigInt(next));
          continue;
        }
      }
      delta = BigInt(deltaNumber);
      previous = BigInt(previousNumber);
      useNumber = false;
    }
    const stepBigInt = typeof step === "number" ? BigInt(step) : step;
    delta = checkedI64(delta + stepBigInt, "delta-of-delta delta");
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
  // Number fast path: differences of safe integers stay exact while the
  // running delta and delta-of-delta remain safe integers.
  let previousDeltaNumber = 0;
  let useNumber = true;
  for (let index = 1; index < values.length; index += 1) {
    const current = values[index]!;
    const prior = values[index - 1]!;
    if (useNumber) {
      if (
        current >= MIN_SAFE_BIGINT &&
        current <= MAX_SAFE_BIGINT &&
        prior >= MIN_SAFE_BIGINT &&
        prior <= MAX_SAFE_BIGINT
      ) {
        const deltaNumber = Number(current) - Number(prior);
        const difference = deltaNumber - previousDeltaNumber;
        if (Number.isSafeInteger(deltaNumber) && Number.isSafeInteger(difference)) {
          previousDeltaNumber = deltaNumber;
          encodeDeltaOfDeltaValueNumber(bits, difference);
          continue;
        }
      }
      previousDelta = BigInt(previousDeltaNumber);
      useNumber = false;
    }
    const delta = checkedI64(current - prior, "delta-of-delta delta");
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
  // Number fast path: safe-integer values encode through the number i128
  // writer, avoiding per-element BigInt subtraction and comparison.
  let previousNumber = 0;
  let useNumber = true;
  for (const input of values) {
    const value = toBigInt(input);
    if (useNumber) {
      if (value >= MIN_SAFE_BIGINT && value <= MAX_SAFE_BIGINT) {
        const valueNumber = Number(value);
        const deltaNumber = valueNumber - previousNumber;
        if (Number.isSafeInteger(deltaNumber)) {
          writePostcardI128Number(writer, deltaNumber);
          previousNumber = valueNumber;
          continue;
        }
      }
      previous = BigInt(previousNumber);
      useNumber = false;
    }
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
  // Number fast path: accumulate with plain number arithmetic while the
  // running value stays a safe integer. A safe integer always compares
  // exactly against these bounds: any bound outside the safe range is
  // replaced by the nearest safe-integer limit, which accepts/rejects the
  // same values as the BigInt comparison.
  let valueNumber = 0;
  let useNumber = true;
  const minNumber = min < MIN_SAFE_BIGINT ? Number.MIN_SAFE_INTEGER : Number(min);
  const maxNumber = max > MAX_SAFE_BIGINT ? Number.MAX_SAFE_INTEGER : Number(max);
  const append = (delta: number | bigint): void => {
    if (useNumber) {
      if (typeof delta === "number") {
        const next = valueNumber + delta;
        if (Number.isSafeInteger(next)) {
          valueNumber = next;
          decodeAssert(
            next >= minNumber && next <= maxNumber,
            "delta RLE value is out of range",
          );
          values.push(fromBigInt(BigInt(next)));
          return;
        }
      }
      value = BigInt(valueNumber);
      useNumber = false;
    }
    const deltaBigInt = typeof delta === "number" ? BigInt(delta) : delta;
    value += deltaBigInt;
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
      const delta = readPostcardI128NumberOrBigInt(reader);
      for (let index = 0; index < length; index += 1) {
        append(delta);
      }
    } else {
      for (let index = 0; index < length; index += 1) {
        append(readPostcardI128NumberOrBigInt(reader));
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
  return readUlebNumber(reader, 0xffff_ffff);
}

function writePostcardU32(writer: ByteWriter, value: number): void {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xffff_ffff) {
    throw new LoroEncodeError(`u32 is out of range: ${value}`);
  }
  writeUleb128(writer, value);
}

function readPostcardU64(reader: ByteReader): bigint {
  return readUleb128(reader, U64_MAX);
}

function writePostcardU64(writer: ByteWriter, value: bigint): void {
  writeUleb128(writer, value);
}

function readPostcardI32(reader: ByteReader): number {
  const encoded = readUlebNumber(reader, 0xffff_ffff);
  // ZigZag decode; every u32-encoded value lands in the i32 range.
  const value = (encoded >>> 1) ^ -(encoded & 1);
  if (value < I32_MIN || value > I32_MAX) {
    throw new LoroDecodeError("postcard signed integer is out of range");
  }
  return value;
}

function writePostcardI32(writer: ByteWriter, value: number): void {
  if (!Number.isSafeInteger(value) || value < I32_MIN || value > I32_MAX) {
    throw new LoroEncodeError(`i32 is out of range: ${value}`);
  }
  writeUleb128(writer, ((value << 1) ^ (value >> 31)) >>> 0);
}

function readPostcardI64(reader: ByteReader): bigint {
  const encoded = readUleb128(reader, U64_MAX);
  const value = (encoded >> 1n) ^ -(encoded & 1n);
  if (value < I64_MIN || value > I64_MAX) {
    throw new LoroDecodeError("postcard signed integer is out of range");
  }
  return value;
}

function writePostcardI64(writer: ByteWriter, value: bigint): void {
  if (value < I64_MIN || value > I64_MAX) {
    throw new LoroEncodeError(`i64 is out of range: ${value}`);
  }
  writeUleb128(writer, (value << 1n) ^ (value >> 63n));
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
      if (encoded < 0x8000_0000) {
        // Smi ZigZag decode; identical to the division-based formula below.
        return (encoded >>> 1) ^ -(encoded & 1);
      }
      return encoded % 2 === 0 ? encoded / 2 : -(encoded + 1) / 2;
    }
    multiplier *= 128;
  }
  throw new LoroDecodeError("u128 varint overflow", start);
}

function writePostcardI128Number(writer: ByteWriter, value: number): void {
  if (Number.isSafeInteger(value) && value > -0x8000_0000 && value < 0x8000_0000) {
    // Smi-only ZigZag encode and varint emission for 31-bit values.
    let encoded = ((value << 1) ^ (value >> 31)) >>> 0;
    do {
      let byte = encoded & 0x7f;
      encoded >>>= 7;
      if (encoded !== 0) {
        byte |= 0x80;
      }
      writer.writeU8(byte);
    } while (encoded !== 0);
    return;
  }
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

// Reads a postcard i128, keeping the whole decode in number arithmetic while
// the encoded varint stays a safe integer and falling back to BigInt only for
// larger (still valid u128) inputs.
function readPostcardI128NumberOrBigInt(reader: ByteReader): number | bigint {
  const start = reader.position;
  let encoded = 0;
  let multiplier = 1;
  let encodedBigInt = 0n;
  let useBigInt = false;
  for (let index = 0; index < 19; index += 1) {
    const byte = reader.readU8();
    const payload = byte & 0x7f;
    if (index === 18 && payload > 3) {
      throw new LoroDecodeError("u128 varint overflow", start);
    }
    if (useBigInt) {
      encodedBigInt |= BigInt(payload) << BigInt(index * 7);
    } else if (payload !== 0) {
      if (multiplier <= Number.MAX_SAFE_INTEGER / payload) {
        const sum = encoded + payload * multiplier;
        if (Number.isSafeInteger(sum)) {
          encoded = sum;
        } else {
          useBigInt = true;
          encodedBigInt = BigInt(encoded) | (BigInt(payload) << BigInt(index * 7));
        }
      } else {
        useBigInt = true;
        encodedBigInt = BigInt(encoded) | (BigInt(payload) << BigInt(index * 7));
      }
    }
    if ((byte & 0x80) === 0) {
      if (!useBigInt) {
        if (encoded < 0x8000_0000) {
          return (encoded >>> 1) ^ -(encoded & 1);
        }
        return encoded % 2 === 0 ? encoded / 2 : -(encoded + 1) / 2;
      }
      const value = (encodedBigInt >> 1n) ^ -(encodedBigInt & 1n);
      decodeAssert(value >= I128_MIN && value <= I128_MAX, "postcard i128 is out of range");
      return value;
    }
    multiplier *= 128;
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
  #bytePosition = 0;
  // Pending bits read from the stream but not yet consumed, right-aligned.
  // The invariant #bitCount <= 7 holds between read calls, so every
  // intermediate below stays a positive 31-bit value and bitwise ops are
  // exact Smi operations.
  #bitBuffer = 0;
  #bitCount = 0;

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
    return this.#bytePosition * 8 - this.#bitCount;
  }

  get remaining(): number {
    return this.#totalBits - this.position;
  }

  // Reads up to 24 bits (the refill loop would exceed 31-bit intermediate
  // values above that) and returns them as a number. Bits are consumed
  // MSB-first, matching the original bit-at-a-time implementation.
  readBitsNumber(count: number): number {
    decodeAssert(
      Number.isSafeInteger(count) && count >= 0 && count <= 24,
      "invalid bit width",
    );
    decodeAssert(this.remaining >= count, "unexpected end of delta-of-delta bitstream");
    while (this.#bitCount < count) {
      this.#bitBuffer = (this.#bitBuffer << 8) | this.#bytes[this.#bytePosition]!;
      this.#bytePosition += 1;
      this.#bitCount += 8;
    }
    const shift = this.#bitCount - count;
    const value = this.#bitBuffer >>> shift;
    this.#bitBuffer &= (1 << shift) - 1;
    this.#bitCount -= count;
    return value;
  }

  readBitsBigInt(count: number): bigint {
    decodeAssert(
      Number.isSafeInteger(count) && count >= 0 && count <= 64,
      "invalid bit width",
    );
    decodeAssert(this.remaining >= count, "unexpected end of delta-of-delta bitstream");
    let value = 0n;
    let remaining = count;
    while (remaining > 0) {
      const step = Math.min(remaining, 24);
      value = (value << BigInt(step)) | BigInt(this.readBitsNumber(step));
      remaining -= step;
    }
    return value;
  }
}

class BitWriter {
  readonly #bytes: number[] = [];
  // Same right-aligned pending-bits representation as BitReader, with the
  // invariant #bitCount <= 7 between write calls.
  #bitBuffer = 0;
  #bitCount = 0;

  // Writes the low `count` bits of value, MSB first. Like the original
  // bit-at-a-time loop, bits of value above `count` are ignored.
  writeBitsNumber(value: number, count: number): void {
    encodeAssert(
      Number.isSafeInteger(count) && count >= 0 && count <= 24,
      "invalid bit width",
    );
    this.#bitBuffer = (this.#bitBuffer << count) | (value & ((1 << count) - 1));
    this.#bitCount += count;
    while (this.#bitCount >= 8) {
      const shift = this.#bitCount - 8;
      this.#bytes.push(this.#bitBuffer >>> shift);
      this.#bitBuffer &= (1 << shift) - 1;
      this.#bitCount -= 8;
    }
  }

  writeBitsBigInt(value: bigint, count: number): void {
    encodeAssert(count >= 0 && count <= 64, "invalid bit width");
    let remaining = count;
    while (remaining > 0) {
      const step = Math.min(remaining, 24);
      remaining -= step;
      this.writeBitsNumber(Number((value >> BigInt(remaining)) & 0xff_ffffn), step);
    }
  }

  finish(): { readonly bytes: Uint8Array; readonly lastUsedBits: number } {
    if (this.#bitCount === 0) {
      return { bytes: Uint8Array.from(this.#bytes), lastUsedBits: 8 };
    }
    this.#bytes.push(this.#bitBuffer << (8 - this.#bitCount));
    return { bytes: Uint8Array.from(this.#bytes), lastUsedBits: this.#bitCount };
  }
}

function decodeDeltaOfDeltaValue(reader: BitReader): number | bigint {
  if (reader.readBitsNumber(1) === 0) {
    return 0;
  }
  if (reader.readBitsNumber(1) === 0) {
    return reader.readBitsNumber(7) - 63;
  }
  if (reader.readBitsNumber(1) === 0) {
    return reader.readBitsNumber(9) - 255;
  }
  if (reader.readBitsNumber(1) === 0) {
    return reader.readBitsNumber(12) - 2047;
  }
  if (reader.readBitsNumber(1) === 0) {
    return reader.readBitsNumber(21) - 1_048_575;
  }
  return BigInt.asIntN(64, reader.readBitsBigInt(64));
}

function encodeDeltaOfDeltaValue(writer: BitWriter, value: bigint): void {
  if (value === 0n) {
    writer.writeBitsNumber(0, 1);
  } else if (value >= -63n && value <= 64n) {
    writer.writeBitsNumber(0b10, 2);
    writer.writeBitsNumber(Number(value + 63n), 7);
  } else if (value >= -255n && value <= 256n) {
    writer.writeBitsNumber(0b110, 3);
    writer.writeBitsNumber(Number(value + 255n), 9);
  } else if (value >= -2047n && value <= 2048n) {
    writer.writeBitsNumber(0b1110, 4);
    writer.writeBitsNumber(Number(value + 2047n), 12);
  } else if (value >= -1_048_575n && value <= 1_048_576n) {
    writer.writeBitsNumber(0b11110, 5);
    writer.writeBitsNumber(Number(value + 1_048_575n), 21);
  } else {
    writer.writeBitsNumber(0b11111, 5);
    writer.writeBitsBigInt(BigInt.asUintN(64, value), 64);
  }
}

function encodeDeltaOfDeltaValueNumber(writer: BitWriter, value: number): void {
  if (value === 0) {
    writer.writeBitsNumber(0, 1);
  } else if (value >= -63 && value <= 64) {
    writer.writeBitsNumber(0b10, 2);
    writer.writeBitsNumber(value + 63, 7);
  } else if (value >= -255 && value <= 256) {
    writer.writeBitsNumber(0b110, 3);
    writer.writeBitsNumber(value + 255, 9);
  } else if (value >= -2047 && value <= 2048) {
    writer.writeBitsNumber(0b1110, 4);
    writer.writeBitsNumber(value + 2047, 12);
  } else if (value >= -1_048_575 && value <= 1_048_576) {
    writer.writeBitsNumber(0b11110, 5);
    writer.writeBitsNumber(value + 1_048_575, 21);
  } else {
    writer.writeBitsNumber(0b11111, 5);
    writer.writeBitsBigInt(BigInt.asUintN(64, BigInt(value)), 64);
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
