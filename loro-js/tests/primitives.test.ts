import { describe, expect, test } from "vitest";

import {
  ByteReader,
  ByteWriter,
  LORO_XXHASH_SEED,
  PostcardReader,
  PostcardWriter,
  bytesEqual,
  bytesToHex,
  readSleb128,
  readUleb128,
  writeSleb128,
  writeUleb128,
  xxhash32,
} from "../src/codec/index";

describe("xxHash32", () => {
  test.each([
    [new Uint8Array(), 0, 0x02cc_5d05],
    [new TextEncoder().encode("a"), 0, 0x550d_7456],
    [new TextEncoder().encode("abc"), 0, 0x32d1_53ff],
    [new TextEncoder().encode("message digest"), 0, 0x7c94_8494],
    [new TextEncoder().encode("abc"), LORO_XXHASH_SEED, 0xa5f8_7ea0],
  ])("matches the reference vector", (input, seed, expected) => {
    expect(xxhash32(input, seed)).toBe(expected);
  });
});

describe("LEB128", () => {
  test.each([0n, 1n, 127n, 128n, 0xffff_ffffn, 0xffff_ffff_ffff_ffffn])(
    "round trips unsigned %s",
    (value) => {
      const writer = new ByteWriter();
      writeUleb128(writer, value);
      const reader = new ByteReader(writer.toUint8Array());
      expect(readUleb128(reader)).toBe(value);
      expect(reader.remaining).toBe(0);
    },
  );

  test.each([-0x8000_0000_0000_0000n, -65n, -1n, 0n, 63n, 64n, 0x7fff_ffff_ffff_ffffn])(
    "round trips signed %s",
    (value) => {
      const writer = new ByteWriter();
      writeSleb128(writer, value);
      const reader = new ByteReader(writer.toUint8Array());
      expect(readSleb128(reader)).toBe(value);
      expect(reader.remaining).toBe(0);
    },
  );

  test("uses signed LEB128 rather than postcard zigzag", () => {
    const writer = new ByteWriter();
    writeSleb128(writer, -1n);
    expect(bytesToHex(writer.toUint8Array())).toBe("7f");
    const postcard = new PostcardWriter();
    postcard.writeI32(-1);
    expect(bytesToHex(postcard.toUint8Array())).toBe("01");
  });
});

describe("postcard primitives", () => {
  test("round trips signed integers, strings, bytes and arrays", () => {
    const writer = new PostcardWriter();
    writer.writeI32(-123);
    writer.writeI64(-9_007_199_254_740_993n);
    writer.writeString("Loro 😀");
    writer.writeBytes(Uint8Array.of(0, 1, 255));
    writer.writeArray([1, 2, 300], (output, value) => output.writeU32(value));

    const reader = new PostcardReader(writer.toUint8Array());
    expect(reader.readI32()).toBe(-123);
    expect(reader.readI64()).toBe(-9_007_199_254_740_993n);
    expect(reader.readString()).toBe("Loro 😀");
    expect(reader.readBytes()).toEqual(Uint8Array.of(0, 1, 255));
    expect(reader.readArray((input) => input.readU32())).toEqual([1, 2, 300]);
    reader.assertEnd();
  });
});

describe("SLEB128 fast paths", () => {
  const reference = (input: bigint): number[] => {
    const bytes: number[] = [];
    let value = input;
    for (;;) {
      let byte = Number(value & 0x7fn);
      value >>= 7n;
      const sign = (byte & 0x40) !== 0;
      const done = (value === 0n && !sign) || (value === -1n && sign);
      if (!done) byte |= 0x80;
      bytes.push(byte);
      if (done) return bytes;
    }
  };

  test("match the BigInt encoding at every width boundary", () => {
    const values: bigint[] = [-(1n << 63n), (1n << 63n) - 1n];
    for (let bits = 0n; bits <= 62n; bits += 1n) {
      for (const delta of [-2n, -1n, 0n, 1n, 2n]) {
        values.push((1n << bits) + delta, -(1n << bits) + delta);
      }
    }
    let state = 0x2545f491;
    for (let index = 0; index < 5000; index += 1) {
      state ^= state << 13;
      state ^= state >>> 17;
      state ^= state << 5;
      values.push(BigInt(state) * BigInt(index + 1));
    }
    const numberMismatches: bigint[] = [];
    for (const value of values) {
      const writer = new ByteWriter();
      writeSleb128(writer, value);
      const bytes = writer.toUint8Array();
      expect([...bytes]).toEqual(reference(value));
      expect(readSleb128(new ByteReader(bytes))).toBe(value);
      if (value <= -(2n ** 53n) || value >= 2n ** 53n) continue;
      const numberWriter = new ByteWriter();
      writeSleb128(numberWriter, Number(value));
      if (!bytesEqual(numberWriter.toUint8Array(), bytes)) numberMismatches.push(value);
    }
    expect(numberMismatches).toEqual([]);
  });
});
