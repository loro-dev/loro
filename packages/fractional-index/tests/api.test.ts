import { describe, expect, test } from "vitest";
import {
  FractionalIndex,
  compare,
  isFractionalIndex,
  newAfter,
  newBefore,
  newBetween,
} from "../src/index";

function lcg(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = Math.imul(state, 1664525) + 1013904223;
    return state >>> 0;
  };
}

describe("FractionalIndex API", () => {
  test("byte arrays are copied on input and output", () => {
    const bytes = new Uint8Array([0x80]);
    const index = FractionalIndex.fromBytes(bytes);
    bytes[0] = 0x00;
    expect(index.toString()).toBe("80");

    const copy = index.toBytes();
    copy[0] = 0x00;
    expect(index.toString()).toBe("80");
  });

  test("JSON and primitive string conversion use Rust-compatible uppercase hex", () => {
    const index = FractionalIndex.fromBytes([0x0f, 0x80, 0xff]);
    expect(index.toString()).toBe("0F80FF");
    expect(index.toJSON()).toBe("0F80FF");
    expect(`${index}`).toBe("0F80FF");
    expect(FractionalIndex.fromHexString("80Z").toString()).toBe("80");
    expect(FractionalIndex.fromHexString("G").toString()).toBe("");
  });

  test("comparison helpers use byte lexicographic order", () => {
    const a = FractionalIndex.fromHexString("7F80");
    const b = FractionalIndex.default();
    const c = FractionalIndex.fromHexString("8180");

    expect(a.compare(b)).toBeLessThan(0);
    expect(b.compare(a)).toBeGreaterThan(0);
    expect(compare(b, FractionalIndex.default())).toBe(0);
    expect(b.equals(FractionalIndex.default())).toBe(true);
    expect([c, a, b].sort(compare).map((x) => x.toString())).toEqual([
      "7F80",
      "80",
      "8180",
    ]);
  });

  test("top-level helpers mirror the class methods", () => {
    const base = FractionalIndex.default();
    expect(newBefore(base).equals(FractionalIndex.newBefore(base))).toBe(true);
    expect(newAfter(base).equals(FractionalIndex.newAfter(base))).toBe(true);
    expect(newBetween(base, FractionalIndex.newAfter(base))?.toString()).toBe(
      FractionalIndex.newBetween(base, FractionalIndex.newAfter(base))?.toString(),
    );
  });

  test("invalid JS values are rejected before they can create non-byte indexes", () => {
    expect(() => FractionalIndex.fromBytes([-1])).toThrow(RangeError);
    expect(() => FractionalIndex.fromBytes([256])).toThrow(RangeError);
    expect(() => FractionalIndex.fromBytes([1.5])).toThrow(RangeError);
    expect(() => FractionalIndex.fromHexString("GG")).toThrow(SyntaxError);
    expect(() => FractionalIndex.generateNEvenly(undefined, undefined, -1)).toThrow(
      RangeError,
    );
    expect(() => FractionalIndex.jitterDefault({ jitter: -1 })).toThrow(RangeError);
    expect(() => FractionalIndex.jitterDefault({ jitter: 256 })).toThrow(RangeError);
    expect(() =>
      FractionalIndex.jitterDefault({ jitter: 1, randomByte: () => 1.5 }),
    ).toThrow(RangeError);
  });

  test("new returns indexes inside the requested open interval", () => {
    const before = FractionalIndex.newBefore(FractionalIndex.default());
    const middle = FractionalIndex.default();
    const after = FractionalIndex.newAfter(middle);

    expect(FractionalIndex.new(undefined, middle)?.compare(middle)).toBeLessThan(0);
    expect(FractionalIndex.new(middle, undefined)?.compare(middle)).toBeGreaterThan(0);

    const leftMiddle = FractionalIndex.new(before, middle);
    expect(leftMiddle?.compare(before)).toBeGreaterThan(0);
    expect(leftMiddle?.compare(middle)).toBeLessThan(0);

    const rightMiddle = FractionalIndex.new(middle, after);
    expect(rightMiddle?.compare(middle)).toBeGreaterThan(0);
    expect(rightMiddle?.compare(after)).toBeLessThan(0);
  });

  test("generateNEvenly returns strictly sorted values within bounds", () => {
    const lower = FractionalIndex.newBefore(FractionalIndex.default());
    const upper = FractionalIndex.newAfter(FractionalIndex.default());
    const values = FractionalIndex.generateNEvenly(lower, upper, 256);

    expect(values).toHaveLength(256);
    expect(values![0]!.compare(lower)).toBeGreaterThan(0);
    expect(values![values!.length - 1]!.compare(upper)).toBeLessThan(0);
    for (let i = 1; i < values!.length; i++) {
      expect(values![i - 1]!.compare(values![i]!)).toBeLessThan(0);
    }
  });

  test("many random insertions remain sorted", () => {
    const next = lcg(0xdecafbad);
    const values = [FractionalIndex.default()];

    for (let i = 0; i < 2_000; i++) {
      const slot = next() % (values.length + 1);
      const lower = slot === 0 ? undefined : values[slot - 1];
      const upper = slot === values.length ? undefined : values[slot];
      const index = FractionalIndex.new(lower, upper);
      expect(index).toBeDefined();
      values.splice(slot, 0, index!);
    }

    for (let i = 1; i < values.length; i++) {
      expect(values[i - 1]!.compare(values[i]!)).toBeLessThan(0);
    }
  });

  test("jitter can use a deterministic random byte source", () => {
    const bytes = [1, 2, 3, 4];
    let offset = 0;
    const randomByte = () => bytes[offset++ % bytes.length]!;

    expect(
      FractionalIndex.jitterDefault({ jitter: 4, randomByte }).toString(),
    ).toBe("8001020304");
  });

  test("runtime type guard recognizes package instances", () => {
    const index = FractionalIndex.default();
    expect(isFractionalIndex(index)).toBe(true);
    expect(isFractionalIndex("80")).toBe(false);
  });
});
