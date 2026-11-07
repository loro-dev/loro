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

/**
 * `n` increasing indexes between the bounds, spread by bisection. Port of
 * Rust's `FractionalIndex::generate_n_evenly`.
 */
export function fractionalIndexesBetween(
  lower: Uint8Array | undefined,
  upper: Uint8Array | undefined,
  n: number,
): Uint8Array[] {
  const output: Uint8Array[] = [];
  const generate = (
    low: Uint8Array | undefined,
    high: Uint8Array | undefined,
    count: number,
  ): void => {
    if (count === 0) return;
    const middle = fractionalIndexBetween(low, high);
    if (count === 1) {
      output.push(middle);
      return;
    }
    const half = Math.floor(count / 2);
    generate(low, middle, half);
    output.push(middle);
    generate(middle, high, count - half - 1);
  };
  generate(lower, upper, n);
  return output;
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
  // Every byte is 0xFF, which a jittered index can end with. Any extension of
  // `bytes` sorts after it, and the caller appends the terminator.
  return [...bytes];
}

function newBetween(left: Uint8Array, right: Uint8Array): number[] | undefined {
  const shorterLength = Math.min(left.length, right.length) - 1;
  for (let index = 0; index < shorterLength; index += 1) {
    const difference = right[index]! - left[index]!;
    if (difference > 0) return newBetweenAt(left, right, index);
    if (difference < 0) return undefined;
  }
  // The last byte of the shorter index. Without jitter it is the terminator,
  // but a jittered index ends with a random byte, so it has to be compared.
  const index = shorterLength;
  const split = shorterLength + 1;
  if (left.length < right.length) {
    if (right[index]! < left[index]!) return undefined;
    const suffix = right.subarray(split);
    if (suffix.every((byte) => byte === 0)) {
      // No terminated index sorts before an all-zero suffix
      return left[index]! < right[index]! ? newBetweenAt(left, right, index) : undefined;
    }
    return [...right.subarray(0, split), ...newBefore(suffix)];
  }
  if (left.length === right.length) {
    return left[index]! < right[index]! ? newBetweenAt(left, right, index) : undefined;
  }
  if (left[index]! >= right[index]!) return undefined;
  return [...left.subarray(0, split), ...newAfter(left.subarray(split))];
}

/** `left` and `right` share their first `index` bytes and `left[index] < right[index]`. */
function newBetweenAt(left: Uint8Array, right: Uint8Array, index: number): number[] {
  const difference = right[index]! - left[index]!;
  if (difference > 1) {
    const output = [...left.subarray(0, index + 1)];
    output[index] = output[index]! + Math.floor(difference / 2);
    return output;
  }
  return [...left.subarray(0, index + 1), ...newAfter(left.subarray(index + 1))];
}
