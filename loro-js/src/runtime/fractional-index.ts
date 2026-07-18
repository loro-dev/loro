import { compareBytes } from "../codec/bytes";

const TERMINATOR = 128;

export function fractionalIndexBetween(
  lower: Uint8Array | undefined,
  upper: Uint8Array | undefined,
): Uint8Array {
  if (lower === undefined && upper === undefined) {
    return Uint8Array.of(TERMINATOR);
  }
  if (lower === undefined) {
    return terminated(newBefore(upper!));
  }
  if (upper === undefined) {
    return terminated(newAfter(lower));
  }
  if (compareBytes(lower, upper) >= 0) {
    throw new RangeError("fractional index bounds are not ordered");
  }
  const between = newBetween(lower, upper);
  if (between === undefined) {
    throw new RangeError("cannot create a fractional index between the bounds");
  }
  return terminated(between);
}

function terminated(bytes: number[]): Uint8Array {
  return Uint8Array.from([...bytes, TERMINATOR]);
}

function newBefore(bytes: Uint8Array): number[] {
  for (let index = 0; index < bytes.length; index += 1) {
    if (bytes[index]! > TERMINATOR) return [...bytes.subarray(0, index)];
    if (bytes[index]! > 0) {
      const output = [...bytes.subarray(0, index + 1)];
      output[index] = output[index]! - 1;
      return output;
    }
  }
  throw new RangeError("invalid fractional index");
}

function newAfter(bytes: Uint8Array): number[] {
  for (let index = 0; index < bytes.length; index += 1) {
    if (bytes[index]! < TERMINATOR) return [...bytes.subarray(0, index)];
    if (bytes[index]! < 255) {
      const output = [...bytes.subarray(0, index + 1)];
      output[index] = output[index]! + 1;
      return output;
    }
  }
  throw new RangeError("invalid fractional index");
}

function newBetween(left: Uint8Array, right: Uint8Array): number[] | undefined {
  const shorterLength = Math.min(left.length, right.length) - 1;
  for (let index = 0; index < shorterLength; index += 1) {
    const difference = right[index]! - left[index]!;
    if (difference > 1) {
      const output = [...left.subarray(0, index + 1)];
      output[index] = output[index]! + Math.floor(difference / 2);
      return output;
    }
    if (difference === 1) {
      return [...left.subarray(0, index + 1), ...newAfter(left.subarray(index + 1))];
    }
    if (difference < 0) return undefined;
  }
  if (left.length < right.length) {
    const split = shorterLength + 1;
    if (right[split - 1]! < TERMINATOR) return undefined;
    return [...right.subarray(0, split), ...newBefore(right.subarray(split))];
  }
  if (left.length > right.length) {
    const split = shorterLength + 1;
    if (left[split - 1]! >= TERMINATOR) return undefined;
    return [...left.subarray(0, split), ...newAfter(left.subarray(split))];
  }
  return undefined;
}
