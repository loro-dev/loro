# @loro-dev/fractional-index

TypeScript implementation of Loro's Rust `loro_fractional_index` crate.

This package creates compact string positions that sort lexicographically. The
public API always accepts and returns canonical uppercase hex strings, so indexes
are easy to store, compare, serialize, and use as object or map keys.

- No runtime dependencies.
- ESM output with TypeScript declarations.
- Public indexes are plain strings.
- Generated strings are byte-for-byte compatible with the Rust crate's
  `Display` output.
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
const before = FractionalIndex.newBefore(first);
const after = FractionalIndex.newAfter(first);
const middle = FractionalIndex.newBetween(first, after);

console.log(first); // "80"
console.log(before); // "7F80"
console.log(after); // "8180"
console.log(middle); // "817F80"

const ordered = [after, first, before].sort(compare);
console.log(ordered); // ["7F80", "80", "8180"]

JSON.stringify({ position: first }); // "{\"position\":\"80\"}"
```

Generated indexes are canonical uppercase hex strings. For generated values,
ordinary string comparison also follows byte order:

```ts
before < first; // true
first < after; // true
```

Use `compare(a, b)` when sorting values that may have been parsed from external
input, because it compares by bytes after parsing.

## Data Model

An index string is the uppercase hex encoding of the same bytes used by Rust.
Generated indexes include the `0x80` terminator byte.

```ts
const index = FractionalIndex.default(); // "80"
```

The public API does not expose byte arrays. Internally, the implementation still
uses bytes to stay aligned with the Rust algorithm.

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
  console.log(left < between); // true for canonical generated strings
  console.log(between < right); // true
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

console.log(values);
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
console.log(index.length); // 8 hex chars: 0x80 plus 3 random bytes
```

For deterministic tests or replicated fixtures, provide `randomByte`:

```ts
const bytes = [1, 2, 3];
let offset = 0;

const index = FractionalIndex.jitterDefault({
  jitter: 3,
  randomByte: () => bytes[offset++],
});

console.log(index); // "80010203"
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
console.log(fromHex); // "80FF"
```

Compatibility notes:

- `fromHexString()` accepts uppercase or lowercase hex and returns canonical
  uppercase hex.
- Like Rust, `fromHexString()` ignores a trailing odd nibble. `"80A"` parses as
  `"80"`.
- Invalid hex pairs throw `SyntaxError`.

## API Reference

```ts
type FractionalIndex = string;

interface JitterOptions {
  jitter?: number;
  randomByte?: () => number;
}
```

### Namespace API

```ts
FractionalIndex.TERMINATOR; // 128

FractionalIndex.default(): FractionalIndex;
FractionalIndex.fromHexString(hex): FractionalIndex;

FractionalIndex.new(lower?, upper?): FractionalIndex | undefined;
FractionalIndex.newBefore(index): FractionalIndex;
FractionalIndex.newAfter(index): FractionalIndex;
FractionalIndex.newBetween(left, right): FractionalIndex | undefined;
FractionalIndex.generateNEvenly(lower, upper, n): FractionalIndex[] | undefined;

FractionalIndex.jitterDefault(options?): FractionalIndex;
FractionalIndex.newJitter(lower, upper, options?): FractionalIndex | undefined;
FractionalIndex.newBeforeJitter(index, options?): FractionalIndex;
FractionalIndex.newAfterJitter(index, options?): FractionalIndex;
FractionalIndex.newBetweenJitter(left, right, options?): FractionalIndex | undefined;
FractionalIndex.generateNEvenlyJitter(
  lower,
  upper,
  n,
  options?,
): FractionalIndex[] | undefined;

FractionalIndex.compare(left, right): number;
FractionalIndex.equals(left, right): boolean;
FractionalIndex.isFractionalIndex(value): value is FractionalIndex;
```

### Top-Level Helpers

Namespace methods are also available as named exports. Since `new` is a
JavaScript keyword, `FractionalIndex.new()` is exported as `newIndex()`:

```ts
defaultIndex();
fromHexString(hex);
compare(left, right);
equals(left, right);
newIndex(lower, upper);
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
- Hex serialization.

## Performance Notes

String indexes are optimized for JavaScript ergonomics: comparison, JSON,
storage, and use as keys are all straightforward. The implementation parses
strings to byte arrays internally for generation so it can stay aligned with the
Rust byte algorithm. For the expected workload of normal list/tree positioning,
this keeps the API simple without a meaningful practical performance cost.
