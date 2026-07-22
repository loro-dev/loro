import { describe, expect, test } from "vitest";

import {
  ByteReader,
  ByteWriter,
  LORO_XXHASH_SEED,
  PostcardReader,
  PostcardWriter,
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

  test.each([0, 1, 127, 128, 0xffff_ffff, Number.MAX_SAFE_INTEGER])(
    "encodes unsigned number %s like bigint",
    (value) => {
      const numberWriter = new ByteWriter();
      writeUleb128(numberWriter, value);
      const bigintWriter = new ByteWriter();
      writeUleb128(bigintWriter, BigInt(value));
      expect(numberWriter.toUint8Array()).toEqual(bigintWriter.toUint8Array());
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

  test.each([Number.MIN_SAFE_INTEGER, -65, -64, -1, 0, 63, 64, Number.MAX_SAFE_INTEGER])(
    "encodes signed number %s like bigint",
    (value) => {
      const numberWriter = new ByteWriter();
      writeSleb128(numberWriter, value);
      const bigintWriter = new ByteWriter();
      writeSleb128(bigintWriter, BigInt(value));
      expect(numberWriter.toUint8Array()).toEqual(bigintWriter.toUint8Array());
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
