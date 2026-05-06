export type FractionalIndex = string;

export interface JitterOptions {
  /**
   * Number of random bytes appended after the 0x80 terminator.
   *
   * Rust's API takes this as a `u8`; JavaScript accepts the same [0, 255]
   * integer range.
   */
  jitter?: number;
  /**
   * Returns one byte in the inclusive range [0, 255].
   *
   * Provide this in tests or replicated systems when deterministic output is
   * required. If omitted, Math.random is used.
   */
  randomByte?: () => number;
}

export interface FractionalIndexNamespace {
  readonly TERMINATOR: 128;
  default(): FractionalIndex;
  fromHexString(hex: string): FractionalIndex;
  "new"(
    lower?: FractionalIndex | null,
    upper?: FractionalIndex | null,
  ): FractionalIndex | undefined;
  newBefore(index: FractionalIndex): FractionalIndex;
  newAfter(index: FractionalIndex): FractionalIndex;
  newBetween(left: FractionalIndex, right: FractionalIndex): FractionalIndex | undefined;
  generateNEvenly(
    lower: FractionalIndex | null | undefined,
    upper: FractionalIndex | null | undefined,
    n: number,
  ): FractionalIndex[] | undefined;
  jitterDefault(options?: JitterOptions): FractionalIndex;
  newJitter(
    lower: FractionalIndex | null | undefined,
    upper: FractionalIndex | null | undefined,
    options?: JitterOptions,
  ): FractionalIndex | undefined;
  newBeforeJitter(index: FractionalIndex, options?: JitterOptions): FractionalIndex;
  newAfterJitter(index: FractionalIndex, options?: JitterOptions): FractionalIndex;
  newBetweenJitter(
    left: FractionalIndex,
    right: FractionalIndex,
    options?: JitterOptions,
  ): FractionalIndex | undefined;
  generateNEvenlyJitter(
    lower: FractionalIndex | null | undefined,
    upper: FractionalIndex | null | undefined,
    n: number,
    options?: JitterOptions,
  ): FractionalIndex[] | undefined;
  compare(left: FractionalIndex, right: FractionalIndex): number;
  equals(left: FractionalIndex, right: FractionalIndex): boolean;
  isFractionalIndex(value: unknown): value is FractionalIndex;
}

const TERMINATOR = 0x80;
const BYTE_MIN = 0x00;
const BYTE_MAX = 0xff;
const DEFAULT_INDEX = "80";
const HEX_TABLE = Array.from({ length: 256 }, (_, i) =>
  i.toString(16).padStart(2, "0").toUpperCase(),
);

export const FractionalIndex: FractionalIndexNamespace = Object.freeze({
  TERMINATOR,
  default: defaultIndex,
  fromHexString,
  new: newIndex,
  newBefore,
  newAfter,
  newBetween,
  generateNEvenly,
  jitterDefault,
  newJitter,
  newBeforeJitter,
  newAfterJitter,
  newBetweenJitter,
  generateNEvenlyJitter,
  compare,
  equals,
  isFractionalIndex,
});

interface NormalizedJitterOptions {
  jitter: number;
  randomByte: () => number;
}

export function defaultIndex(): FractionalIndex {
  return DEFAULT_INDEX;
}

export function fromHexString(hex: string): FractionalIndex {
  return bytesToHexUnchecked(hexToBytes(hex));
}

export function newIndex(
  lower?: FractionalIndex | null,
  upper?: FractionalIndex | null,
): FractionalIndex | undefined {
  const value = newBytes(
    optionalIndexBytes(lower, "lower"),
    optionalIndexBytes(upper, "upper"),
  );
  return value ? bytesToHexUnchecked(value) : undefined;
}

export function newBefore(index: FractionalIndex): FractionalIndex {
  return bytesToHexUnchecked(
    fromUnterminatedBytes(newBeforeBytes(parseIndex(index, "index"))),
  );
}

export function newAfter(index: FractionalIndex): FractionalIndex {
  return bytesToHexUnchecked(
    fromUnterminatedBytes(newAfterBytes(parseIndex(index, "index"))),
  );
}

export function newBetween(
  left: FractionalIndex,
  right: FractionalIndex,
): FractionalIndex | undefined {
  const bytes = newBetweenBytes(parseIndex(left, "left"), parseIndex(right, "right"));
  return bytes ? bytesToHexUnchecked(fromUnterminatedBytes(bytes)) : undefined;
}

export function generateNEvenly(
  lower: FractionalIndex | null | undefined,
  upper: FractionalIndex | null | undefined,
  n: number,
): FractionalIndex[] | undefined {
  assertNonNegativeSafeInteger(n, "n");

  if (n === 0) {
    return [];
  }

  const lowerBytes = optionalIndexBytes(lower, "lower");
  const upperBytes = optionalIndexBytes(upper, "upper");
  if (lowerBytes && upperBytes && compareBytes(lowerBytes, upperBytes) >= 0) {
    return undefined;
  }

  const values: FractionalIndex[] = new Array(n);
  let offset = 0;
  const push = (value: Uint8Array) => {
    values[offset++] = bytesToHexUnchecked(value);
  };

  const generate = (
    lo: Uint8Array | undefined,
    hi: Uint8Array | undefined,
    count: number,
  ) => {
    if (count === 0) {
      return;
    }

    const mid = Math.floor(count / 2);
    const midValue = newBytes(lo, hi);
    if (!midValue) {
      throw new Error("newBytes returned undefined inside generateNEvenly");
    }

    if (count === 1) {
      push(midValue);
      return;
    }

    generate(lo, midValue, mid);
    push(midValue);

    const rightCount = count - mid - 1;
    if (rightCount !== 0) {
      generate(midValue, hi, rightCount);
    }
  };

  generate(lowerBytes, upperBytes, n);
  return values;
}

export function jitterDefault(options: JitterOptions = {}): FractionalIndex {
  return bytesToHexUnchecked(jitterBytes(new Uint8Array(0), options));
}

export function newJitter(
  lower: FractionalIndex | null | undefined,
  upper: FractionalIndex | null | undefined,
  options: JitterOptions = {},
): FractionalIndex | undefined {
  const normalized = normalizeJitterOptions(options);
  const value = newJitterBytes(
    optionalIndexBytes(lower, "lower"),
    optionalIndexBytes(upper, "upper"),
    normalized,
  );
  return value ? bytesToHexUnchecked(value) : undefined;
}

export function newBeforeJitter(
  index: FractionalIndex,
  options: JitterOptions = {},
): FractionalIndex {
  return bytesToHexUnchecked(
    jitterBytes(newBeforeBytes(parseIndex(index, "index")), options),
  );
}

export function newAfterJitter(
  index: FractionalIndex,
  options: JitterOptions = {},
): FractionalIndex {
  return bytesToHexUnchecked(
    jitterBytes(newAfterBytes(parseIndex(index, "index")), options),
  );
}

export function newBetweenJitter(
  left: FractionalIndex,
  right: FractionalIndex,
  options: JitterOptions = {},
): FractionalIndex | undefined {
  const bytes = newBetweenBytes(parseIndex(left, "left"), parseIndex(right, "right"));
  return bytes ? bytesToHexUnchecked(jitterBytes(bytes, options)) : undefined;
}

export function generateNEvenlyJitter(
  lower: FractionalIndex | null | undefined,
  upper: FractionalIndex | null | undefined,
  n: number,
  options: JitterOptions = {},
): FractionalIndex[] | undefined {
  assertNonNegativeSafeInteger(n, "n");

  if (n === 0) {
    return [];
  }

  const lowerBytes = optionalIndexBytes(lower, "lower");
  const upperBytes = optionalIndexBytes(upper, "upper");
  if (lowerBytes && upperBytes && compareBytes(lowerBytes, upperBytes) >= 0) {
    return undefined;
  }

  const normalized = normalizeJitterOptions(options);
  const values: FractionalIndex[] = new Array(n);
  let offset = 0;
  const push = (value: Uint8Array) => {
    values[offset++] = bytesToHexUnchecked(value);
  };

  const generate = (
    lo: Uint8Array | undefined,
    hi: Uint8Array | undefined,
    count: number,
  ) => {
    if (count === 0) {
      return;
    }

    const mid = Math.floor(count / 2);
    const midValue = newJitterBytes(lo, hi, normalized);
    if (!midValue) {
      throw new Error("newJitterBytes returned undefined inside generateNEvenlyJitter");
    }

    if (count === 1) {
      push(midValue);
      return;
    }

    generate(lo, midValue, mid);
    push(midValue);

    const rightCount = count - mid - 1;
    if (rightCount !== 0) {
      generate(midValue, hi, rightCount);
    }
  };

  generate(lowerBytes, upperBytes, n);
  return values;
}

export function compare(left: FractionalIndex, right: FractionalIndex): number {
  return compareBytes(parseIndex(left, "left"), parseIndex(right, "right"));
}

export function equals(left: FractionalIndex, right: FractionalIndex): boolean {
  return compare(left, right) === 0;
}

export function isFractionalIndex(value: unknown): value is FractionalIndex {
  if (typeof value !== "string" || value.length === 0 || value.length % 2 !== 0) {
    return false;
  }

  for (let i = 0; i < value.length; i++) {
    const code = value.charCodeAt(i);
    const isDigit = code >= 48 && code <= 57;
    const isUpperHex = code >= 65 && code <= 70;
    if (!isDigit && !isUpperHex) {
      return false;
    }
  }

  return true;
}

export const TERMINATOR_BYTE = TERMINATOR;

function newBytes(
  lower: Uint8Array | undefined,
  upper: Uint8Array | undefined,
): Uint8Array | undefined {
  if (lower && upper) {
    const bytes = newBetweenBytes(lower, upper);
    return bytes ? fromUnterminatedBytes(bytes) : undefined;
  }

  if (lower) {
    return fromUnterminatedBytes(newAfterBytes(lower));
  }

  if (upper) {
    return fromUnterminatedBytes(newBeforeBytes(upper));
  }

  return new Uint8Array([TERMINATOR]);
}

function newJitterBytes(
  lower: Uint8Array | undefined,
  upper: Uint8Array | undefined,
  options: NormalizedJitterOptions,
): Uint8Array | undefined {
  if (lower && upper) {
    const bytes = newBetweenBytes(lower, upper);
    return bytes ? jitterBytesWithNormalized(bytes, options) : undefined;
  }

  if (lower) {
    return jitterBytesWithNormalized(newAfterBytes(lower), options);
  }

  if (upper) {
    return jitterBytesWithNormalized(newBeforeBytes(upper), options);
  }

  return jitterBytesWithNormalized(new Uint8Array(0), options);
}

function fromUnterminatedBytes(bytes: Uint8Array): Uint8Array {
  const output = new Uint8Array(bytes.length + 1);
  output.set(bytes);
  output[bytes.length] = TERMINATOR;
  return output;
}

function jitterBytes(bytes: Uint8Array, options: JitterOptions): Uint8Array {
  return jitterBytesWithNormalized(bytes, normalizeJitterOptions(options));
}

function jitterBytesWithNormalized(
  bytes: Uint8Array,
  options: NormalizedJitterOptions,
): Uint8Array {
  const output = new Uint8Array(bytes.length + 1 + options.jitter);
  output.set(bytes);
  output[bytes.length] = TERMINATOR;

  for (let i = 0; i < options.jitter; i++) {
    output[bytes.length + 1 + i] = readRandomByte(options.randomByte);
  }

  return output;
}

function optionalIndexBytes(
  value: FractionalIndex | null | undefined,
  name: string,
): Uint8Array | undefined {
  if (value == null) {
    return undefined;
  }

  return parseIndex(value, name);
}

function parseIndex(value: FractionalIndex, name: string): Uint8Array {
  if (typeof value !== "string") {
    throw new TypeError(`${name} must be a fractional index string`);
  }

  return hexToBytes(value);
}

function hexToBytes(hex: string): Uint8Array {
  if (typeof hex !== "string") {
    throw new TypeError("hex must be a string");
  }

  const length = Math.floor(hex.length / 2);
  const output = new Uint8Array(length);
  for (let i = 0; i < length; i++) {
    const offset = i * 2;
    const high = parseHexNibble(hex.charCodeAt(offset), offset);
    const low = parseHexNibble(hex.charCodeAt(offset + 1), offset + 1);
    output[i] = high * 16 + low;
  }

  return output;
}

function parseHexNibble(code: number, offset: number): number {
  if (code >= 48 && code <= 57) {
    return code - 48;
  }

  if (code >= 65 && code <= 70) {
    return code - 55;
  }

  if (code >= 97 && code <= 102) {
    return code - 87;
  }

  throw new SyntaxError(`invalid hex character at offset ${offset}`);
}

function bytesToHexUnchecked(bytes: Uint8Array): string {
  const parts = new Array<string>(bytes.length);
  for (let i = 0; i < bytes.length; i++) {
    parts[i] = HEX_TABLE[bytes[i]!]!;
  }

  return parts.join("");
}

function newBeforeBytes(bytes: Uint8Array): Uint8Array {
  for (let i = 0; i < bytes.length; i++) {
    const byte = bytes[i]!;
    if (byte > TERMINATOR) {
      return bytes.slice(0, i);
    }

    if (byte > BYTE_MIN) {
      const output = bytes.slice(0, i + 1);
      output[i] = byte - 1;
      return output;
    }
  }

  throw new Error("internal error: entered unreachable code");
}

function newAfterBytes(bytes: Uint8Array): Uint8Array {
  for (let i = 0; i < bytes.length; i++) {
    const byte = bytes[i]!;
    if (byte < TERMINATOR) {
      return bytes.slice(0, i);
    }

    if (byte < BYTE_MAX) {
      const output = bytes.slice(0, i + 1);
      output[i] = byte + 1;
      return output;
    }
  }

  throw new Error("internal error: entered unreachable code");
}

function newBetweenBytes(left: Uint8Array, right: Uint8Array): Uint8Array | undefined {
  const minLen = Math.min(left.length, right.length);
  if (minLen === 0) {
    throw new Error("attempt to subtract with overflow");
  }

  const shorterLen = minLen - 1;
  for (let i = 0; i < shorterLen; i++) {
    const leftByte = left[i]!;
    const rightByte = right[i]!;
    const diff = rightByte - leftByte;

    if (diff > 1) {
      const output = left.slice(0, i + 1);
      output[i] = leftByte + Math.floor(diff / 2);
      return output;
    }

    if (diff === 1) {
      const prefix = left.slice(0, i + 1);
      const suffix = left.slice(i + 1);
      return concatBytes(prefix, newAfterBytes(suffix));
    }

    if (diff < 0) {
      return undefined;
    }
  }

  if (left.length < right.length) {
    const split = shorterLen + 1;
    const prefix = right.slice(0, split);
    if (prefix[prefix.length - 1]! < TERMINATOR) {
      return undefined;
    }

    return concatBytes(prefix, newBeforeBytes(right.slice(split)));
  }

  if (left.length === right.length) {
    return undefined;
  }

  const split = shorterLen + 1;
  const prefix = left.slice(0, split);
  if (prefix[prefix.length - 1]! >= TERMINATOR) {
    return undefined;
  }

  return concatBytes(prefix, newAfterBytes(left.slice(split)));
}

function concatBytes(left: Uint8Array, right: Uint8Array): Uint8Array {
  const output = new Uint8Array(left.length + right.length);
  output.set(left);
  output.set(right, left.length);
  return output;
}

function compareBytes(left: Uint8Array, right: Uint8Array): number {
  const minLen = Math.min(left.length, right.length);
  for (let i = 0; i < minLen; i++) {
    const diff = left[i]! - right[i]!;
    if (diff !== 0) {
      return diff;
    }
  }

  return left.length - right.length;
}

function normalizeJitterOptions(options: JitterOptions): NormalizedJitterOptions {
  assertByte(options.jitter ?? 0, "jitter");
  if (options.randomByte !== undefined && typeof options.randomByte !== "function") {
    throw new TypeError("randomByte must be a function");
  }

  return {
    jitter: options.jitter ?? 0,
    randomByte: options.randomByte ?? defaultRandomByte,
  };
}

function defaultRandomByte(): number {
  return Math.floor(Math.random() * 256);
}

function readRandomByte(randomByte: () => number): number {
  const byte = randomByte();
  assertByte(byte, "randomByte()");
  return byte;
}

function assertByte(value: unknown, name: string): asserts value is number {
  if (
    typeof value !== "number" ||
    !Number.isInteger(value) ||
    value < BYTE_MIN ||
    value > BYTE_MAX
  ) {
    throw new RangeError(`${name} must be an integer byte in [0, 255]`);
  }
}

function assertNonNegativeSafeInteger(
  value: unknown,
  name: string,
): asserts value is number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new RangeError(`${name} must be a non-negative safe integer`);
  }
}
