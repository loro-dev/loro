import { describe, expect, test } from "vitest";

import {
  decodeAnyRleU32,
  decodeBoolRle,
  decodeColumnarVec,
  decodeDeltaOfDeltaI64,
  decodeDeltaRleI32,
  decodeDeltaRleIsize,
  decodeDeltaRleU32,
  decodeDeltaRleUsize,
  decodeRleU8,
  encodeBoolRle,
  encodeColumnarVec,
  encodeDeltaOfDeltaI64,
  encodeDeltaRleI32,
  encodeDeltaRleIsize,
  encodeDeltaRleU32,
  encodeDeltaRleUsize,
  takeAnyRleU32,
  takeBoolRle,
  takeDeltaOfDeltaI64,
  bytesToHex,
} from "../src/codec/index";

describe("serde-columnar", () => {
  test("matches the outer vector format", () => {
    const encoded = encodeColumnarVec([
      new TextEncoder().encode("abc"),
      Uint8Array.of(1, 2),
    ]);
    expect(bytesToHex(encoded)).toBe("0203616263020102");
    expect(decodeColumnarVec(encoded)).toEqual([
      new TextEncoder().encode("abc"),
      Uint8Array.of(1, 2),
    ]);
  });

  test("decodes known BoolRle and AnyRle runs", () => {
    expect(decodeBoolRle(Uint8Array.of(0, 2, 3, 1))).toEqual([
      true,
      true,
      false,
      false,
      false,
      true,
    ]);
    expect(decodeRleU8(Uint8Array.of(6, 5, 4, 3))).toEqual([5, 5, 5, 3, 3]);
    expect(decodeAnyRleU32(Uint8Array.of(5, 1, 2, 3))).toEqual([1, 2, 3]);
  });

  test("supports prefix decoding from concatenated streams", () => {
    const boolBytes = Uint8Array.of(0, 2, 3, 1, 99);
    const [bools, boolRest] = takeBoolRle(boolBytes, 6);
    expect(bools).toEqual([true, true, false, false, false, true]);
    expect(boolRest).toEqual(Uint8Array.of(99));

    const anyBytes = Uint8Array.of(5, 1, 2, 3, 99);
    const [values, anyRest] = takeAnyRleU32(anyBytes, 3);
    expect(values).toEqual([1, 2, 3]);
    expect(anyRest).toEqual(Uint8Array.of(99));
  });

  test("round trips BoolRle", () => {
    const values = [false, false, true, true, false, true, true, true];
    expect(decodeBoolRle(encodeBoolRle(values))).toEqual(values);
  });

  test("decodes the known DeltaRle run encoding", () => {
    expect(decodeDeltaRleU32(Uint8Array.of(2, 20, 6, 2, 4, 4))).toEqual([
      10, 11, 12, 13, 15, 17,
    ]);
  });

  test("preserves the canonical literal DeltaRle encoding", () => {
    expect(bytesToHex(encodeDeltaRleU32([10, 11, 12, 13, 15, 17]))).toBe(
      "0b140202020404",
    );
    expect(bytesToHex(encodeDeltaRleI32([-2, -1, 0, 5, 3]))).toBe("090302020a03");
  });

  test("round trips the full i32 and u32 delta range", () => {
    const unsigned = [0, 0xffff_ffff, 0, 0xffff_ffff];
    const signed = [-0x8000_0000, 0x7fff_ffff, -0x8000_0000];
    expect(decodeDeltaRleU32(encodeDeltaRleU32(unsigned))).toEqual(unsigned);
    expect(decodeDeltaRleI32(encodeDeltaRleI32(signed))).toEqual(signed);
  });

  test("round trips DeltaRle integer variants", () => {
    const unsigned = [0, 1, 2, 10, 11, 5];
    const signed = [-2, -1, 0, 5, 3];
    const wideUnsigned = unsigned.map(BigInt);
    const wideSigned = signed.map(BigInt);
    expect(decodeDeltaRleU32(encodeDeltaRleU32(unsigned))).toEqual(unsigned);
    expect(decodeDeltaRleI32(encodeDeltaRleI32(signed))).toEqual(signed);
    expect(decodeDeltaRleUsize(encodeDeltaRleUsize(wideUnsigned))).toEqual(wideUnsigned);
    expect(decodeDeltaRleIsize(encodeDeltaRleIsize(wideSigned))).toEqual(wideSigned);
  });

  test.each([
    { values: [] as bigint[], hex: "0000" },
    { values: [5n], hex: "010a00" },
    { values: [1n, 2n, 3n], hex: "010202a000" },
  ])("matches known DeltaOfDelta bytes $hex", ({ values, hex }) => {
    const encoded = encodeDeltaOfDeltaI64(values);
    expect(bytesToHex(encoded)).toBe(hex);
    expect(decodeDeltaOfDeltaI64(encoded)).toEqual(values);
  });

  test("takes a DeltaOfDelta prefix without consuming the next stream", () => {
    const encoded = encodeDeltaOfDeltaI64([10n, 12n, 15n]);
    const combined = new Uint8Array(encoded.length + 2);
    combined.set(encoded);
    combined.set([7, 8], encoded.length);
    const [values, rest] = takeDeltaOfDeltaI64(combined, 3);
    expect(values).toEqual([10n, 12n, 15n]);
    expect(rest).toEqual(Uint8Array.of(7, 8));
  });
});
