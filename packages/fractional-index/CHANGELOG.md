# @loro-dev/fractional-index

## 0.2.1

### Patch Changes

- 8932e46: Fix a panic when creating or moving a tree node between siblings whose fractional indexes were generated with jitter (#1211). `newBetween` now also compares the last byte of the shorter index, which jitter makes random, and `newAfter` accepts an all-0xFF suffix. Indexes generated without jitter are unchanged.

## 0.2.0

### Minor Changes

- 418d4ca: Add a TypeScript fractional index package aligned with the Rust `loro_fractional_index` crate.
