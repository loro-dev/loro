import { describe, expect, test } from "vitest";
import fixture from "./fixtures/rust-golden.json";
import { FractionalIndex, compare } from "../src/index";

type MaybeHex = string | null;

function idx(hex: string): FractionalIndex {
  return FractionalIndex.fromHexString(hex);
}

function maybeToHex(value: FractionalIndex | undefined): MaybeHex {
  return value?.toString() ?? null;
}

function byteRng(byte: number): () => number {
  return () => byte;
}

describe("Rust golden fixtures", () => {
  test("basic construction and hex encoding match Rust", () => {
    expect(FractionalIndex.TERMINATOR).toBe(fixture.basic.terminator);
    expect(FractionalIndex.default().toString()).toBe(fixture.basic.default);
    expect(FractionalIndex.newBefore(idx("80")).toString()).toBe(
      fixture.basic.beforeDefault,
    );
    expect(FractionalIndex.newAfter(idx("80")).toString()).toBe(
      fixture.basic.afterDefault,
    );
    expect(FractionalIndex.fromHexString("80ffA").toString()).toBe(
      fixture.basic.fromHexOddLength,
    );
    expect(FractionalIndex.fromBytes([0, 15, 128, 255]).toString()).toBe(
      fixture.basic.fromBytes,
    );
  });

  test("newAfter chains match Rust", () => {
    let current = FractionalIndex.default();
    for (const expected of fixture.chains.after) {
      current = FractionalIndex.newAfter(current);
      expect(current.toString()).toBe(expected);
    }
  });

  test("newBefore chains match Rust", () => {
    let current = FractionalIndex.default();
    for (const expected of fixture.chains.before) {
      current = FractionalIndex.newBefore(current);
      expect(current.toString()).toBe(expected);
    }
  });

  test("new handles all lower/upper combinations like Rust", () => {
    for (const c of fixture.newCases) {
      const lower = c.lower == null ? undefined : idx(c.lower);
      const upper = c.upper == null ? undefined : idx(c.upper);
      expect(maybeToHex(FractionalIndex.new(lower, upper))).toBe(c.value);
    }
  });

  test("newBetween matches Rust edge cases", () => {
    for (const c of fixture.between) {
      const run = () => FractionalIndex.newBetween(idx(c.left), idx(c.right));
      if (c.panics) {
        expect(run).toThrow();
      } else {
        expect(maybeToHex(run())).toBe(c.value);
      }
    }
  });

  test("generateNEvenly matches Rust for unbounded, bounded, and invalid ranges", () => {
    for (const c of fixture.evenly) {
      const lower = c.lower == null ? undefined : idx(c.lower);
      const upper = c.upper == null ? undefined : idx(c.upper);
      const value = FractionalIndex.generateNEvenly(lower, upper, c.n);
      expect(value?.map((x) => x.toString()) ?? null).toEqual(c.value);
    }
  });

  test("jitter APIs append random bytes in the same positions as Rust", () => {
    expect(
      FractionalIndex.jitterDefault({
        jitter: fixture.jitter.defaultJitter0.jitter,
        randomByte: byteRng(fixture.jitter.defaultJitter0.byte),
      }).toString(),
    ).toBe(fixture.jitter.defaultJitter0.value);

    expect(
      FractionalIndex.jitterDefault({
        jitter: fixture.jitter.defaultJitter3.jitter,
        randomByte: byteRng(fixture.jitter.defaultJitter3.byte),
      }).toString(),
    ).toBe(fixture.jitter.defaultJitter3.value);

    expect(
      FractionalIndex.newBeforeJitter(idx(fixture.jitter.before.input), {
        jitter: fixture.jitter.before.jitter,
        randomByte: byteRng(fixture.jitter.before.byte),
      }).toString(),
    ).toBe(fixture.jitter.before.value);

    expect(
      FractionalIndex.newAfterJitter(idx(fixture.jitter.after.input), {
        jitter: fixture.jitter.after.jitter,
        randomByte: byteRng(fixture.jitter.after.byte),
      }).toString(),
    ).toBe(fixture.jitter.after.value);

    expect(
      maybeToHex(
        FractionalIndex.newBetweenJitter(
          idx(fixture.jitter.between.lower),
          idx(fixture.jitter.between.upper),
          {
            jitter: fixture.jitter.between.jitter,
            randomByte: byteRng(fixture.jitter.between.byte),
          },
        ),
      ),
    ).toBe(fixture.jitter.between.value);

    expect(
      maybeToHex(
        FractionalIndex.newJitter(undefined, undefined, {
          jitter: fixture.jitter.newNoneNone.jitter,
          randomByte: byteRng(fixture.jitter.newNoneNone.byte),
        }),
      ),
    ).toBe(fixture.jitter.newNoneNone.value);

    expect(
      maybeToHex(
        FractionalIndex.newJitter(idx(fixture.jitter.newAfter.lower), undefined, {
          jitter: fixture.jitter.newAfter.jitter,
          randomByte: byteRng(fixture.jitter.newAfter.byte),
        }),
      ),
    ).toBe(fixture.jitter.newAfter.value);

    expect(
      maybeToHex(
        FractionalIndex.newJitter(undefined, idx(fixture.jitter.newBefore.upper), {
          jitter: fixture.jitter.newBefore.jitter,
          randomByte: byteRng(fixture.jitter.newBefore.byte),
        }),
      ),
    ).toBe(fixture.jitter.newBefore.value);

    const generated = FractionalIndex.generateNEvenlyJitter(
      undefined,
      undefined,
      fixture.jitter.generateN.n,
      {
        jitter: fixture.jitter.generateN.jitter,
        randomByte: byteRng(fixture.jitter.generateN.byte),
      },
    );
    expect(generated?.map((x) => x.toString()) ?? null).toEqual(
      fixture.jitter.generateN.value,
    );
  });

  test("fixture outputs are ordered with the package comparator", () => {
    for (const c of fixture.evenly) {
      if (!Array.isArray(c.value)) {
        continue;
      }

      for (let i = 1; i < c.value.length; i++) {
        expect(compare(idx(c.value[i - 1]!), idx(c.value[i]!))).toBeLessThan(0);
      }
    }
  });
});
