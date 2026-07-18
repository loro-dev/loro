import { ByteReader, ByteWriter } from "./bytes";
import { LoroDecodeError, LoroEncodeError } from "./errors";

export const U64_MAX = 0xffff_ffff_ffff_ffffn;
export const I64_MIN = -0x8000_0000_0000_0000n;
export const I64_MAX = 0x7fff_ffff_ffff_ffffn;

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
  const start = reader.position;
  let result = 0;
  let multiplier = 1;
  // Set once the value provably exceeds Number.MAX_SAFE_INTEGER (and therefore
  // any max). Bytes are still consumed with readUleb128's overflow rules so
  // the reader position and error behavior match the BigInt implementation.
  let exceedsSafeInteger = false;
  for (let index = 0; index < 10; index += 1) {
    const byte = reader.readU8();
    const payload = byte & 0x7f;
    if (index === 9 && payload > 1) {
      throw new LoroDecodeError("ULEB128 overflow", start);
    }
    if (!exceedsSafeInteger && payload !== 0) {
      if (multiplier > Number.MAX_SAFE_INTEGER / payload) {
        exceedsSafeInteger = true;
      } else {
        const product = payload * multiplier;
        if (result > Number.MAX_SAFE_INTEGER - product) {
          exceedsSafeInteger = true;
        } else {
          result += product;
        }
      }
    }
    if ((byte & 0x80) === 0) {
      if (exceedsSafeInteger || result > max) {
        throw new LoroDecodeError("ULEB128 value is out of range", start);
      }
      return result;
    }
    multiplier *= 128;
  }
  throw new LoroDecodeError("ULEB128 overflow", start);
}

export function writeUleb128(writer: ByteWriter, input: bigint | number): void {
  if (
    typeof input === "number" &&
    Number.isSafeInteger(input) &&
    input >= 0 &&
    input < 0x8000_0000
  ) {
    // Smi fast path: encode small numbers without any BigInt arithmetic.
    let value = input;
    do {
      let byte = value & 0x7f;
      value >>>= 7;
      if (value !== 0) {
        byte |= 0x80;
      }
      writer.writeU8(byte);
    } while (value !== 0);
    return;
  }
  let value = typeof input === "number" ? numberToBigInt(input, "ULEB128") : input;
  if (value < 0n || value > U64_MAX) {
    throw new LoroEncodeError(`ULEB128 value is out of range: ${value}`);
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
  let value = typeof input === "number" ? numberToBigInt(input, "SLEB128") : input;
  if (value < I64_MIN || value > I64_MAX) {
    throw new LoroEncodeError(`SLEB128 value is out of range: ${value}`);
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

function numberToBigInt(value: number, name: string): bigint {
  if (!Number.isSafeInteger(value)) {
    throw new LoroEncodeError(`${name} number must be a safe integer: ${value}`);
  }
  return BigInt(value);
}
