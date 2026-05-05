# @loro-dev/fractional-index

TypeScript implementation of Loro's Rust `loro_fractional_index` crate.

It creates compact byte-string positions that sort lexicographically. This is
useful when a list or tree needs stable order keys and callers want to insert
new items before, after, or between existing items without renumbering the whole
collection.

The package is intentionally small:

- No runtime dependencies.
- ESM output with TypeScript declarations.
- Uses `Uint8Array` internally.
- Hex strings are byte-for-byte compatible with the Rust crate's `Display`
  output.
- Golden tests are generated from the Rust implementation in this repository.

## Install

```sh
pnpm add @loro-dev/fractional-index
```

```sh
npm install @loro-dev/fractional-index
```

```sh
yarn add @loro-dev/fractional-index
```

## Quick Start

```ts
import { FractionalIndex, compare } from "@loro-dev/fractional-index";

const first = FractionalIndex.default();
console.log(first.toString()); // "80"

const before = FractionalIndex.newBefore(first);
const after = FractionalIndex.newAfter(first);
const middle = FractionalIndex.newBetween(first, after);

console.log(before.toString()); // "7F80"
console.log(after.toString()); // "8180"
console.log(middle?.toString()); // "817F80"

const ordered = [after, first, before].sort(compare);
console.log(ordered.map(String)); // ["7F80", "80", "8180"]
```

## Data Model

A fractional index is an immutable wrapper around bytes. Valid indexes generated
by this package include the same `0x80` terminator byte used by Rust.

```ts
const index = FractionalIndex.default();

index.toBytes(); // Uint8Array [0x80]
index.toString(); // "80"
JSON.stringify(index); // "\"80\""
```

Ordering is byte lexicographic order, not locale string order and not numeric
order. Use `index.compare(other)` or the exported `compare(a, b)` helper.

## Creating Indexes

### Default

```ts
const index = FractionalIndex.default(); // "80"
```

This matches `FractionalIndex::default()` in Rust.

### Before And After

```ts
const base = FractionalIndex.default();

const before = FractionalIndex.newBefore(base); // "7F80"
const after = FractionalIndex.newAfter(base); // "8180"
```

### Between Two Indexes

```ts
const left = FractionalIndex.default();
const right = FractionalIndex.newAfter(left);

const between = FractionalIndex.newBetween(left, right);
if (between) {
  console.log(left.compare(between) < 0);
  console.log(between.compare(right) < 0);
}
```

`newBetween()` returns `undefined` when the Rust crate would return `None`.
For byte sequences that trigger a Rust panic, this package throws an `Error`
instead of returning a misleading value.

### General Constructor

`FractionalIndex.new(lower, upper)` mirrors Rust's `FractionalIndex::new`:

```ts
FractionalIndex.new(undefined, undefined); // default "80"
FractionalIndex.new(existing, undefined); // after existing
FractionalIndex.new(undefined, existing); // before existing
FractionalIndex.new(left, right); // between left and right
```

`null` and `undefined` are both treated as an absent bound.

## Generating Many Indexes

Use `generateNEvenly()` when you know how many positions you need. It produces
strictly sorted indexes inside the open interval `(lower, upper)`.

```ts
const values = FractionalIndex.generateNEvenly(undefined, undefined, 5);

console.log(values?.map(String));
// ["7E80", "7F80", "80", "817F80", "8180"]
```

Bounded generation:

```ts
const lower = FractionalIndex.newBefore(FractionalIndex.default());
const upper = FractionalIndex.newAfter(FractionalIndex.default());

const values = FractionalIndex.generateNEvenly(lower, upper, 100);
```

The method returns `undefined` if both bounds are provided and `lower >= upper`.

## Jitter

The Rust crate can append random bytes after the terminator. This package exposes
the same behavior through `JitterOptions`.

```ts
const index = FractionalIndex.jitterDefault({ jitter: 3 });
console.log(index.length); // 4 bytes: 0x80 plus 3 random bytes
```

For deterministic tests or replicated fixtures, provide `randomByte`:

```ts
const bytes = [1, 2, 3];
let offset = 0;

const index = FractionalIndex.jitterDefault({
  jitter: 3,
  randomByte: () => bytes[offset++],
});

console.log(index.toString()); // "80010203"
```

Jitter variants:

```ts
FractionalIndex.jitterDefault({ jitter: 2 });
FractionalIndex.newJitter(lower, upper, { jitter: 2 });
FractionalIndex.newBeforeJitter(index, { jitter: 2 });
FractionalIndex.newAfterJitter(index, { jitter: 2 });
FractionalIndex.newBetweenJitter(left, right, { jitter: 2 });
FractionalIndex.generateNEvenlyJitter(lower, upper, 10, { jitter: 2 });
```

`jitter` and `randomByte` must both be integers in `[0, 255]`, matching Rust's
`u8` boundary.

## Parsing And Serialization

```ts
const fromHex = FractionalIndex.fromHexString("80ff");
console.log(fromHex.toString()); // "80FF"

const fromBytes = FractionalIndex.fromBytes(new Uint8Array([0x80]));
console.log(fromBytes.toString()); // "80"
```

Compatibility notes:

- `fromHexString()` accepts uppercase or lowercase hex.
- Like Rust, `fromHexString()` ignores a trailing odd nibble. `"80A"` parses as
  `"80"`.
- Invalid hex pairs throw `SyntaxError`.
- `fromBytes()` copies input bytes and rejects non-byte JavaScript numbers.
- `toBytes()` and `asBytes()` return a copy.

## API Reference

### Class

```ts
interface JitterOptions {
  jitter?: number;
  randomByte?: () => number;
}

class FractionalIndex {
  static readonly TERMINATOR: 128;

  static default(): FractionalIndex;
  static fromBytes(bytes: Uint8Array | readonly number[]): FractionalIndex;
  static fromHexString(hex: string): FractionalIndex;

  static new(
    lower?: FractionalIndex | null,
    upper?: FractionalIndex | null,
  ): FractionalIndex | undefined;
  static newBefore(index: FractionalIndex): FractionalIndex;
  static newAfter(index: FractionalIndex): FractionalIndex;
  static newBetween(
    left: FractionalIndex,
    right: FractionalIndex,
  ): FractionalIndex | undefined;

  static generateNEvenly(
    lower: FractionalIndex | null | undefined,
    upper: FractionalIndex | null | undefined,
    n: number,
  ): FractionalIndex[] | undefined;

  static jitterDefault(options?: JitterOptions): FractionalIndex;
  static newJitter(
    lower: FractionalIndex | null | undefined,
    upper: FractionalIndex | null | undefined,
    options?: JitterOptions,
  ): FractionalIndex | undefined;
  static newBeforeJitter(
    index: FractionalIndex,
    options?: JitterOptions,
  ): FractionalIndex;
  static newAfterJitter(
    index: FractionalIndex,
    options?: JitterOptions,
  ): FractionalIndex;
  static newBetweenJitter(
    left: FractionalIndex,
    right: FractionalIndex,
    options?: JitterOptions,
  ): FractionalIndex | undefined;
  static generateNEvenlyJitter(
    lower: FractionalIndex | null | undefined,
    upper: FractionalIndex | null | undefined,
    n: number,
    options?: JitterOptions,
  ): FractionalIndex[] | undefined;

  readonly length: number;
  toBytes(): Uint8Array;
  asBytes(): Uint8Array;
  compare(other: FractionalIndex): number;
  equals(other: FractionalIndex): boolean;
  toString(): string;
  toJSON(): string;
}
```

### Top-Level Helpers

```ts
compare(left, right);
newBefore(index);
newAfter(index);
newBetween(left, right);
generateNEvenly(lower, upper, n);
jitterDefault(options);
newJitter(lower, upper, options);
newBeforeJitter(index, options);
newAfterJitter(index, options);
newBetweenJitter(left, right, options);
generateNEvenlyJitter(lower, upper, n, options);
bytesToHex(bytes);
isFractionalIndex(value);
```

## Testing Against Rust

The golden fixture used by the TypeScript tests is generated from the Rust crate:

```sh
pnpm --filter @loro-dev/fractional-index fixtures:generate
pnpm --filter @loro-dev/fractional-index check
```

The fixture covers:

- Default, before, after, and between generation.
- Long before/after chains.
- `generate_n_evenly` for bounded and unbounded ranges.
- `None` cases.
- Rust panic edge cases.
- Jitter byte placement.
- Hex and byte serialization.

## Performance Notes

Generated indexes are small `Uint8Array` values. The implementation avoids
decimal arithmetic, `BigInt`, string parsing during comparisons, and runtime
dependencies. Prefer storing `FractionalIndex` instances while actively editing
or sorting, and serialize to hex only at API/storage boundaries.
