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

function terminated(bytes: Uint8Array): Uint8Array {
  const output = new Uint8Array(bytes.length + 1);
  output.set(bytes);
  output[bytes.length] = TERMINATOR;
  return output;
}

function newBefore(bytes: Uint8Array): Uint8Array {
  for (let index = 0; index < bytes.length; index += 1) {
    if (bytes[index]! > TERMINATOR) return bytes.slice(0, index);
    if (bytes[index]! > 0) {
      const output = bytes.slice(0, index + 1);
      output[index] = output[index]! - 1;
      return output;
    }
  }
  throw new RangeError("invalid fractional index");
}

function newAfter(bytes: Uint8Array): Uint8Array {
  for (let index = 0; index < bytes.length; index += 1) {
    if (bytes[index]! < TERMINATOR) return bytes.slice(0, index);
    if (bytes[index]! < 255) {
      const output = bytes.slice(0, index + 1);
      output[index] = output[index]! + 1;
      return output;
    }
  }
  throw new RangeError("invalid fractional index");
}

function newBetween(left: Uint8Array, right: Uint8Array): Uint8Array | undefined {
  const shorterLength = Math.min(left.length, right.length) - 1;
  for (let index = 0; index < shorterLength; index += 1) {
    const difference = right[index]! - left[index]!;
    if (difference > 1) {
      const output = left.slice(0, index + 1);
      output[index] = output[index]! + Math.floor(difference / 2);
      return output;
    }
    if (difference === 1) {
      const tail = newAfter(left.subarray(index + 1));
      const output = new Uint8Array(index + 1 + tail.length);
      output.set(left.subarray(0, index + 1));
      output.set(tail, index + 1);
      return output;
    }
    if (difference < 0) return undefined;
  }
  if (left.length < right.length) {
    const split = shorterLength + 1;
    if (right[split - 1]! < TERMINATOR) return undefined;
    const tail = newBefore(right.subarray(split));
    const output = new Uint8Array(split + tail.length);
    output.set(right.subarray(0, split));
    output.set(tail, split);
    return output;
  }
  if (left.length > right.length) {
    const split = shorterLength + 1;
    if (left[split - 1]! >= TERMINATOR) return undefined;
    const tail = newAfter(left.subarray(split));
    const output = new Uint8Array(split + tail.length);
    output.set(left.subarray(0, split));
    output.set(tail, split);
    return output;
  }
  return undefined;
}
