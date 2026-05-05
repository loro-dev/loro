export type BytesLike = Uint8Array | readonly number[];

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

const TERMINATOR = 0x80;
const BYTE_MIN = 0x00;
const BYTE_MAX = 0xff;
const HEX_TABLE = Array.from({ length: 256 }, (_, i) =>
  i.toString(16).padStart(2, "0").toUpperCase(),
);

export class FractionalIndex {
  static readonly TERMINATOR = TERMINATOR;

  private static readonly DEFAULT_INDEX = new FractionalIndex(
    new Uint8Array([TERMINATOR]),
  );

  private readonly bytes_: Uint8Array;

  private constructor(bytes: Uint8Array) {
    this.bytes_ = bytes;
  }

  static default(): FractionalIndex {
    return FractionalIndex.DEFAULT_INDEX;
  }

  static fromBytes(bytes: BytesLike): FractionalIndex {
    return new FractionalIndex(copyBytes(bytes));
  }

  static fromHexString(hex: string): FractionalIndex {
    return new FractionalIndex(hexToBytes(hex));
  }

  static new(
    lower?: FractionalIndex | null,
    upper?: FractionalIndex | null,
  ): FractionalIndex | undefined {
    const lowerIndex = optionalIndex(lower, "lower");
    const upperIndex = optionalIndex(upper, "upper");

    if (lowerIndex && upperIndex) {
      return FractionalIndex.newBetween(lowerIndex, upperIndex);
    }

    if (lowerIndex) {
      return FractionalIndex.newAfter(lowerIndex);
    }

    if (upperIndex) {
      return FractionalIndex.newBefore(upperIndex);
    }

    return FractionalIndex.default();
  }

  static newBefore(index: FractionalIndex): FractionalIndex {
    assertFractionalIndex(index, "index");
    return FractionalIndex.fromUnterminated(newBeforeBytes(index.bytes_));
  }

  static newAfter(index: FractionalIndex): FractionalIndex {
    assertFractionalIndex(index, "index");
    return FractionalIndex.fromUnterminated(newAfterBytes(index.bytes_));
  }

  static newBetween(
    left: FractionalIndex,
    right: FractionalIndex,
  ): FractionalIndex | undefined {
    assertFractionalIndex(left, "left");
    assertFractionalIndex(right, "right");
    const bytes = newBetweenBytes(left.bytes_, right.bytes_);
    return bytes ? FractionalIndex.fromUnterminated(bytes) : undefined;
  }

  static generateNEvenly(
    lower: FractionalIndex | null | undefined,
    upper: FractionalIndex | null | undefined,
    n: number,
  ): FractionalIndex[] | undefined {
    assertNonNegativeSafeInteger(n, "n");

    if (n === 0) {
      return [];
    }

    const lowerIndex = optionalIndex(lower, "lower");
    const upperIndex = optionalIndex(upper, "upper");
    if (lowerIndex && upperIndex && lowerIndex.compare(upperIndex) >= 0) {
      return undefined;
    }

    const values: FractionalIndex[] = new Array(n);
    let offset = 0;
    const push = (value: FractionalIndex) => {
      values[offset++] = value;
    };

    const generate = (
      lo: FractionalIndex | undefined,
      hi: FractionalIndex | undefined,
      count: number,
    ) => {
      if (count === 0) {
        return;
      }

      const mid = Math.floor(count / 2);
      const midValue = FractionalIndex.new(lo, hi);
      if (!midValue) {
        throw new Error("FractionalIndex.new returned undefined inside generateNEvenly");
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

    generate(lowerIndex, upperIndex, n);
    return values;
  }

  static jitterDefault(options: JitterOptions = {}): FractionalIndex {
    return FractionalIndex.jitter(new Uint8Array(0), options);
  }

  static newJitter(
    lower: FractionalIndex | null | undefined,
    upper: FractionalIndex | null | undefined,
    options: JitterOptions = {},
  ): FractionalIndex | undefined {
    const lowerIndex = optionalIndex(lower, "lower");
    const upperIndex = optionalIndex(upper, "upper");

    if (lowerIndex && upperIndex) {
      return FractionalIndex.newBetweenJitter(lowerIndex, upperIndex, options);
    }

    if (lowerIndex) {
      return FractionalIndex.newAfterJitter(lowerIndex, options);
    }

    if (upperIndex) {
      return FractionalIndex.newBeforeJitter(upperIndex, options);
    }

    return FractionalIndex.jitterDefault(options);
  }

  static newBeforeJitter(
    index: FractionalIndex,
    options: JitterOptions = {},
  ): FractionalIndex {
    assertFractionalIndex(index, "index");
    return FractionalIndex.jitter(newBeforeBytes(index.bytes_), options);
  }

  static newAfterJitter(
    index: FractionalIndex,
    options: JitterOptions = {},
  ): FractionalIndex {
    assertFractionalIndex(index, "index");
    return FractionalIndex.jitter(newAfterBytes(index.bytes_), options);
  }

  static newBetweenJitter(
    left: FractionalIndex,
    right: FractionalIndex,
    options: JitterOptions = {},
  ): FractionalIndex | undefined {
    assertFractionalIndex(left, "left");
    assertFractionalIndex(right, "right");
    const bytes = newBetweenBytes(left.bytes_, right.bytes_);
    return bytes ? FractionalIndex.jitter(bytes, options) : undefined;
  }

  static generateNEvenlyJitter(
    lower: FractionalIndex | null | undefined,
    upper: FractionalIndex | null | undefined,
    n: number,
    options: JitterOptions = {},
  ): FractionalIndex[] | undefined {
    assertNonNegativeSafeInteger(n, "n");

    if (n === 0) {
      return [];
    }

    const lowerIndex = optionalIndex(lower, "lower");
    const upperIndex = optionalIndex(upper, "upper");
    if (lowerIndex && upperIndex && lowerIndex.compare(upperIndex) >= 0) {
      return undefined;
    }

    const normalized = normalizeJitterOptions(options);
    const values: FractionalIndex[] = new Array(n);
    let offset = 0;
    const push = (value: FractionalIndex) => {
      values[offset++] = value;
    };

    const generate = (
      lo: FractionalIndex | undefined,
      hi: FractionalIndex | undefined,
      count: number,
    ) => {
      if (count === 0) {
        return;
      }

      const mid = Math.floor(count / 2);
      const midValue = FractionalIndex.newJitterWithNormalized(
        lo,
        hi,
        normalized,
      );
      if (!midValue) {
        throw new Error(
          "FractionalIndex.newJitter returned undefined inside generateNEvenlyJitter",
        );
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

    generate(lowerIndex, upperIndex, n);
    return values;
  }

  get length(): number {
    return this.bytes_.length;
  }

  toBytes(): Uint8Array {
    return this.bytes_.slice();
  }

  asBytes(): Uint8Array {
    return this.toBytes();
  }

  compare(other: FractionalIndex): number {
    assertFractionalIndex(other, "other");
    return compareBytes(this.bytes_, other.bytes_);
  }

  equals(other: FractionalIndex): boolean {
    return isFractionalIndex(other) && compareBytes(this.bytes_, other.bytes_) === 0;
  }

  toString(): string {
    return bytesToHexUnchecked(this.bytes_);
  }

  toJSON(): string {
    return this.toString();
  }

  valueOf(): string {
    return this.toString();
  }

  [Symbol.toPrimitive](): string {
    return this.toString();
  }

  private static fromUnterminated(bytes: Uint8Array): FractionalIndex {
    const output = new Uint8Array(bytes.length + 1);
    output.set(bytes);
    output[bytes.length] = TERMINATOR;
    return new FractionalIndex(output);
  }

  private static jitter(
    bytes: Uint8Array,
    options: JitterOptions,
  ): FractionalIndex {
    const normalized = normalizeJitterOptions(options);
    return FractionalIndex.jitterWithNormalized(bytes, normalized);
  }

  private static jitterWithNormalized(
    bytes: Uint8Array,
    options: NormalizedJitterOptions,
  ): FractionalIndex {
    const output = new Uint8Array(bytes.length + 1 + options.jitter);
    output.set(bytes);
    output[bytes.length] = TERMINATOR;

    for (let i = 0; i < options.jitter; i++) {
      output[bytes.length + 1 + i] = readRandomByte(options.randomByte);
    }

    return new FractionalIndex(output);
  }

  private static newJitterWithNormalized(
    lower: FractionalIndex | undefined,
    upper: FractionalIndex | undefined,
    options: NormalizedJitterOptions,
  ): FractionalIndex | undefined {
    if (lower && upper) {
      const bytes = newBetweenBytes(lower.bytes_, upper.bytes_);
      return bytes ? FractionalIndex.jitterWithNormalized(bytes, options) : undefined;
    }

    if (lower) {
      return FractionalIndex.jitterWithNormalized(
        newAfterBytes(lower.bytes_),
        options,
      );
    }

    if (upper) {
      return FractionalIndex.jitterWithNormalized(
        newBeforeBytes(upper.bytes_),
        options,
      );
    }

    return FractionalIndex.jitterWithNormalized(new Uint8Array(0), options);
  }
}

interface NormalizedJitterOptions {
  jitter: number;
  randomByte: () => number;
}

export function isFractionalIndex(value: unknown): value is FractionalIndex {
  return value instanceof FractionalIndex;
}

export function compare(left: FractionalIndex, right: FractionalIndex): number {
  assertFractionalIndex(left, "left");
  return left.compare(right);
}

export function newBefore(index: FractionalIndex): FractionalIndex {
  return FractionalIndex.newBefore(index);
}

export function newAfter(index: FractionalIndex): FractionalIndex {
  return FractionalIndex.newAfter(index);
}

export function newBetween(
  left: FractionalIndex,
  right: FractionalIndex,
): FractionalIndex | undefined {
  return FractionalIndex.newBetween(left, right);
}

export function generateNEvenly(
  lower: FractionalIndex | null | undefined,
  upper: FractionalIndex | null | undefined,
  n: number,
): FractionalIndex[] | undefined {
  return FractionalIndex.generateNEvenly(lower, upper, n);
}

export function jitterDefault(options: JitterOptions = {}): FractionalIndex {
  return FractionalIndex.jitterDefault(options);
}

export function newJitter(
  lower: FractionalIndex | null | undefined,
  upper: FractionalIndex | null | undefined,
  options: JitterOptions = {},
): FractionalIndex | undefined {
  return FractionalIndex.newJitter(lower, upper, options);
}

export function newBeforeJitter(
  index: FractionalIndex,
  options: JitterOptions = {},
): FractionalIndex {
  return FractionalIndex.newBeforeJitter(index, options);
}

export function newAfterJitter(
  index: FractionalIndex,
  options: JitterOptions = {},
): FractionalIndex {
  return FractionalIndex.newAfterJitter(index, options);
}

export function newBetweenJitter(
  left: FractionalIndex,
  right: FractionalIndex,
  options: JitterOptions = {},
): FractionalIndex | undefined {
  return FractionalIndex.newBetweenJitter(left, right, options);
}

export function generateNEvenlyJitter(
  lower: FractionalIndex | null | undefined,
  upper: FractionalIndex | null | undefined,
  n: number,
  options: JitterOptions = {},
): FractionalIndex[] | undefined {
  return FractionalIndex.generateNEvenlyJitter(lower, upper, n, options);
}

export function bytesToHex(bytes: BytesLike): string {
  return bytesToHexUnchecked(copyBytes(bytes));
}

export const TERMINATOR_BYTE = TERMINATOR;

function copyBytes(bytes: BytesLike): Uint8Array {
  if (bytes instanceof Uint8Array) {
    return bytes.slice();
  }

  if (!Array.isArray(bytes)) {
    throw new TypeError("bytes must be a Uint8Array or an array of byte values");
  }

  const output = new Uint8Array(bytes.length);
  for (let i = 0; i < bytes.length; i++) {
    const byte = bytes[i];
    assertByte(byte, `bytes[${i}]`);
    output[i] = byte;
  }

  return output;
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

function newBetweenBytes(
  left: Uint8Array,
  right: Uint8Array,
): Uint8Array | undefined {
  const minLen = Math.min(left.length, right.length);
  if (minLen === 0) {
    throw new Error("attempt to subtract with overflow");
  }

  const shorterLen = minLen - 1;
  for (let i = 0; i < shorterLen; i++) {
    const leftByte = left[i]!;
    const rightByte = right[i]!;

    if (rightByte === BYTE_MIN) {
      throw new Error("attempt to subtract with overflow");
    }

    if (leftByte < rightByte - 1) {
      const output = left.slice(0, i + 1);
      output[i] = leftByte + Math.floor((rightByte - leftByte) / 2);
      return output;
    }

    if (leftByte === rightByte - 1) {
      const prefix = left.slice(0, i + 1);
      const suffix = left.slice(i + 1);
      return concatBytes(prefix, newAfterBytes(suffix));
    }

    if (leftByte > rightByte) {
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

function optionalIndex(
  value: FractionalIndex | null | undefined,
  name: string,
): FractionalIndex | undefined {
  if (value == null) {
    return undefined;
  }

  assertFractionalIndex(value, name);
  return value;
}

function assertFractionalIndex(
  value: unknown,
  name: string,
): asserts value is FractionalIndex {
  if (!isFractionalIndex(value)) {
    throw new TypeError(`${name} must be a FractionalIndex`);
  }
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

function assertNonNegativeSafeInteger(value: unknown, name: string): asserts value is number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < 0
  ) {
    throw new RangeError(`${name} must be a non-negative safe integer`);
  }
}
