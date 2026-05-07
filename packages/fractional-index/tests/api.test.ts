import { describe, expect, test } from "vitest";
import type { FractionalIndex as FractionalIndexString } from "../src/index";
import {
  FractionalIndex,
  compare,
  defaultIndex,
  equals,
  isFractionalIndex,
  newAfter,
  newBefore,
  newBetween,
  newIndex,
} from "../src/index";

function lcg(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = Math.imul(state, 1664525) + 1013904223;
    return state >>> 0;
  };
}

describe("FractionalIndex string API", () => {
  test("public indexes are plain strings", () => {
    const index: FractionalIndexString = FractionalIndex.default();

    expect(index).toBe("80");
    expect(typeof index).toBe("string");
    expect(JSON.stringify(index)).toBe('"80"');
    expect(index < FractionalIndex.newAfter(index)).toBe(true);
  });

  test("JSON and primitive string conversion use Rust-compatible uppercase hex", () => {
    const index = FractionalIndex.fromHexString("0f80ff");
    expect(index).toBe("0F80FF");
    expect(JSON.stringify(index)).toBe('"0F80FF"');
    expect(FractionalIndex.fromHexString("80ffA")).toBe("80FF");
    expect(FractionalIndex.fromHexString("80Z")).toBe("80");
    expect(FractionalIndex.fromHexString("G")).toBe("");
  });

  test("comparison helpers use byte lexicographic order", () => {
    const a = FractionalIndex.fromHexString("7F80");
    const b = FractionalIndex.default();
    const c = FractionalIndex.fromHexString("8180");

    expect(compare(a, b)).toBeLessThan(0);
    expect(compare(b, a)).toBeGreaterThan(0);
    expect(compare(b, FractionalIndex.default())).toBe(0);
    expect(equals(b, FractionalIndex.default())).toBe(true);
    expect([c, a, b].sort(compare)).toEqual(["7F80", "80", "8180"]);
  });

  test("top-level helpers mirror the namespace methods", () => {
    const base = FractionalIndex.default();
    expect(defaultIndex()).toBe(FractionalIndex.default());
    expect(newIndex(base, undefined)).toBe(FractionalIndex.new(base, undefined));
    expect(newBefore(base)).toBe(FractionalIndex.newBefore(base));
    expect(newAfter(base)).toBe(FractionalIndex.newAfter(base));
    expect(newBetween(base, FractionalIndex.newAfter(base))).toBe(
      FractionalIndex.newBetween(base, FractionalIndex.newAfter(base)),
    );
  });

  test("invalid JS values are rejected before they can create invalid indexes", () => {
    expect(() => FractionalIndex.fromHexString("GG")).toThrow(SyntaxError);
    expect(() => FractionalIndex.generateNEvenly(undefined, undefined, -1)).toThrow(
      RangeError,
    );
    expect(() => FractionalIndex.jitterDefault({ jitter: -1 })).toThrow(RangeError);
    expect(() => FractionalIndex.jitterDefault({ jitter: 256 })).toThrow(RangeError);
    expect(() =>
      FractionalIndex.jitterDefault({ jitter: 1, randomByte: () => 1.5 }),
    ).toThrow(RangeError);
    expect(() => FractionalIndex.newBefore(80 as unknown as string)).toThrow(TypeError);
  });

  test("public namespace does not expose byte-array helpers", () => {
    expect("fromBytes" in FractionalIndex).toBe(false);
    expect("toBytes" in FractionalIndex).toBe(false);
    expect("bytesToHex" in FractionalIndex).toBe(false);
  });

  test("new returns indexes inside the requested open interval", () => {
    const before = FractionalIndex.newBefore(FractionalIndex.default());
    const middle = FractionalIndex.default();
    const after = FractionalIndex.newAfter(middle);

    expect(compare(FractionalIndex.new(undefined, middle)!, middle)).toBeLessThan(0);
    expect(compare(FractionalIndex.new(middle, undefined)!, middle)).toBeGreaterThan(0);

    const leftMiddle = FractionalIndex.new(before, middle);
    expect(compare(leftMiddle!, before)).toBeGreaterThan(0);
    expect(compare(leftMiddle!, middle)).toBeLessThan(0);

    const rightMiddle = FractionalIndex.new(middle, after);
    expect(compare(rightMiddle!, middle)).toBeGreaterThan(0);
    expect(compare(rightMiddle!, after)).toBeLessThan(0);
  });

  test("newBetween handles generated indexes with a zero shared prefix", () => {
    let upper = FractionalIndex.default();
    for (let i = 0; i < 127; i++) {
      upper = FractionalIndex.newBefore(upper);
    }

    const lower = FractionalIndex.newBefore(upper);
    expect(lower).toBe("0080");
    expect(upper).toBe("0180");

    const middle = FractionalIndex.newBetween(lower, upper);
    expect(middle).toBe("008180");

    const between = FractionalIndex.newBetween(lower, middle!);
    expect(between).toBe("00817F80");
    expect(compare(between!, lower)).toBeGreaterThan(0);
    expect(compare(between!, middle!)).toBeLessThan(0);

    const jittered = FractionalIndex.newBetweenJitter(lower, middle!, {
      jitter: 1,
      randomByte: () => 0xaa,
    });
    expect(jittered).toBe("00817F80AA");
  });

  test("generateNEvenly returns strictly sorted values within bounds", () => {
    const lower = FractionalIndex.newBefore(FractionalIndex.default());
    const upper = FractionalIndex.newAfter(FractionalIndex.default());
    const values = FractionalIndex.generateNEvenly(lower, upper, 256);

    expect(values).toHaveLength(256);
    expect(compare(values![0]!, lower)).toBeGreaterThan(0);
    expect(compare(values![values!.length - 1]!, upper)).toBeLessThan(0);
    for (let i = 1; i < values!.length; i++) {
      expect(compare(values![i - 1]!, values![i]!)).toBeLessThan(0);
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
      expect(compare(values[i - 1]!, values[i]!)).toBeLessThan(0);
    }
  });

  test("jitter can use a deterministic random byte source", () => {
    const bytes = [1, 2, 3, 4];
    let offset = 0;
    const randomByte = () => bytes[offset++ % bytes.length]!;

    expect(FractionalIndex.jitterDefault({ jitter: 4, randomByte })).toBe("8001020304");
  });

  test("runtime type guard recognizes canonical fractional index strings", () => {
    expect(isFractionalIndex("80")).toBe(true);
    expect(isFractionalIndex("80FF")).toBe(true);
    expect(isFractionalIndex("80ff")).toBe(false);
    expect(isFractionalIndex("80Z")).toBe(false);
    expect(isFractionalIndex("GG")).toBe(false);
    expect(isFractionalIndex("")).toBe(false);
    expect(isFractionalIndex(80)).toBe(false);
  });
});
