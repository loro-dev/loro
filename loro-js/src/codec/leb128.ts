import { ByteReader, ByteWriter } from "./bytes";
import { LoroDecodeError, LoroEncodeError } from "./errors";

export const U64_MAX = 0xffff_ffff_ffff_ffffn;
export const I64_MIN = -0x8000_0000_0000_0000n;
export const I64_MAX = 0x7fff_ffff_ffff_ffffn;
const MAX_SAFE_BIGINT = BigInt(Number.MAX_SAFE_INTEGER);
const MIN_SAFE_BIGINT = BigInt(Number.MIN_SAFE_INTEGER);

export function readUleb128(reader: ByteReader, max = U64_MAX): bigint {
  let result = 0n;
  let shift = 0n;
  const start = reader.position;
  for (let index = 0; index < 10; index += 1) {
    const byte = reader.readU8();
    const payload = BigInt(byte & 0x7f);
    if (shift === 63n && payload > 1n) {
      throw new LoroDecodeError("ULEB128 overflow", start);
    }
    result |= payload << shift;
    if ((byte & 0x80) === 0) {
      if (result > max) {
        throw new LoroDecodeError("ULEB128 value is out of range", start);
      }
      return result;
    }
    shift += 7n;
  }
  throw new LoroDecodeError("ULEB128 overflow", start);
}

export function readUlebNumber(
  reader: ByteReader,
  max = Number.MAX_SAFE_INTEGER,
): number {
  if (!Number.isSafeInteger(max) || max < 0) {
    throw new LoroDecodeError("invalid ULEB128 number limit", reader.position);
  }
  const value = readUleb128(reader, BigInt(max));
  return Number(value);
}

export function writeUleb128(writer: ByteWriter, input: bigint | number): void {
  if (typeof input === "number") {
    if (!Number.isSafeInteger(input)) {
      throw new LoroEncodeError(`ULEB128 number must be a safe integer: ${input}`);
    }
    if (input < 0) {
      throw new LoroEncodeError(`ULEB128 value is out of range: ${input}`);
    }
    writeUlebNumber(writer, input);
    return;
  }

  let value = input;
  if (value < 0n || value > U64_MAX) {
    throw new LoroEncodeError(`ULEB128 value is out of range: ${value}`);
  }
  if (value <= MAX_SAFE_BIGINT) {
    writeUlebNumber(writer, Number(value));
    return;
  }
  do {
    let byte = Number(value & 0x7fn);
    value >>= 7n;
    if (value !== 0n) {
      byte |= 0x80;
    }
    writer.writeU8(byte);
  } while (value !== 0n);
}

export function readSleb128(reader: ByteReader): bigint {
  let result = 0n;
  let shift = 0n;
  let byte = 0;
  const start = reader.position;
  for (let index = 0; index < 10; index += 1) {
    byte = reader.readU8();
    const payload = BigInt(byte & 0x7f);
    result |= payload << shift;
    shift += 7n;
    if ((byte & 0x80) === 0) {
      if (index === 9) {
        if (byte !== 0 && byte !== 0x7f) {
          throw new LoroDecodeError("SLEB128 overflow", start);
        }
        return BigInt.asIntN(64, result);
      }
      if ((byte & 0x40) !== 0) {
        result |= -1n << shift;
      }
      if (result < I64_MIN || result > I64_MAX) {
        throw new LoroDecodeError("SLEB128 value is out of range", start);
      }
      return result;
    }
  }
  throw new LoroDecodeError("SLEB128 overflow", start);
}

export function writeSleb128(writer: ByteWriter, input: bigint | number): void {
  if (typeof input === "number") {
    if (!Number.isSafeInteger(input)) {
      throw new LoroEncodeError(`SLEB128 number must be a safe integer: ${input}`);
    }
    writeSlebNumber(writer, input);
    return;
  }

  let value = input;
  if (value < I64_MIN || value > I64_MAX) {
    throw new LoroEncodeError(`SLEB128 value is out of range: ${value}`);
  }
  if (value >= MIN_SAFE_BIGINT && value <= MAX_SAFE_BIGINT) {
    writeSlebNumber(writer, Number(value));
    return;
  }
  for (;;) {
    let byte = Number(value & 0x7fn);
    value >>= 7n;
    const sign = (byte & 0x40) !== 0;
    const done = (value === 0n && !sign) || (value === -1n && sign);
    if (!done) {
      byte |= 0x80;
    }
    writer.writeU8(byte);
    if (done) {
      return;
    }
  }
}

function writeUlebNumber(writer: ByteWriter, input: number): void {
  let value = input;
  do {
    let byte = value % 128;
    value = Math.floor(value / 128);
    if (value !== 0) {
      byte |= 0x80;
    }
    writer.writeU8(byte);
  } while (value !== 0);
}

function writeSlebNumber(writer: ByteWriter, input: number): void {
  let value = input;
  for (;;) {
    let byte = ((value % 128) + 128) % 128;
    value = Math.floor(value / 128);
    const sign = (byte & 0x40) !== 0;
    const done = (value === 0 && !sign) || (value === -1 && sign);
    if (!done) {
      byte |= 0x80;
    }
    writer.writeU8(byte);
    if (done) {
      return;
    }
  }
}
