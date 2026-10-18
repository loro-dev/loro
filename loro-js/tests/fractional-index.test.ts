import { describe, expect, test } from "vitest";

import { bytesToHex, hexToBytes } from "../src/codec/bytes";
import { fractionalIndexBetween } from "../src/runtime/fractional-index";

// Expected values are what Rust's FractionalIndex::new_between returns.

describe("fractional index between jittered positions", () => {
  // A Rust peer with fractional-index jitter gives positions that end with
  // random bytes (loro-dev/loro#1211).
  test.each([
    ["8180FF", "818180CB", "8180FF80"],
    ["80FF", "8180", "80FF80"],
    ["8180AA", "8180CC", "8180BB80"],
    ["8180AA", "8180AB", "8180AA80"],
    ["809011", "80A0", "809080"],
    ["8010", "8020", "801880"],
    ["8010", "80200000", "801880"],
  ])("between %s and %s", (lower, upper, expected) => {
    const between = fractionalIndexBetween(hexToBytes(lower), hexToBytes(upper));
    expect(bytesToHex(between).toUpperCase()).toBe(expected);
  });

  test("throws when nothing lies between the bounds", () => {
    expect(() =>
      fractionalIndexBetween(hexToBytes("8010"), hexToBytes("80100000")),
    ).toThrow(RangeError);
  });
});
