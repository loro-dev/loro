import { describe, expect, test } from "vitest";

import {
  EncodeMode,
  LoroDecodeError,
  bytesToHex,
  decodeDocument,
  decodeFastSnapshot,
  decodeFastUpdates,
  encodeDocument,
  encodeFastSnapshot,
  encodeFastUpdates,
  encodeFastUpdatesBody,
} from "../src/codec/index";

describe("Loro document envelope", () => {
  test("covers mode and body with the checksum", () => {
    const encoded = encodeDocument(EncodeMode.FastUpdates, Uint8Array.of(1, 2, 3));
    expect(bytesToHex(encoded.subarray(0, 4))).toBe("6c6f726f");
    expect(encoded.subarray(4, 16)).toEqual(new Uint8Array(12));
    expect(encoded.subarray(20, 22)).toEqual(Uint8Array.of(0, 4));
    expect(decodeDocument(encoded)).toMatchObject({ mode: EncodeMode.FastUpdates });

    const corrupted = encoded.slice();
    corrupted[20] = corrupted[20]! ^ 1;
    expect(() => decodeDocument(corrupted)).toThrow(LoroDecodeError);
  });
});

describe("FastSnapshot", () => {
  test("round trips all three exact sections", () => {
    const snapshot = {
      oplog: Uint8Array.of(1, 2),
      state: Uint8Array.of(3),
      shallowRootState: Uint8Array.of(4, 5, 6),
    };
    expect(decodeFastSnapshot(encodeFastSnapshot(snapshot))).toEqual(snapshot);
  });

  test("rejects trailing section bytes", () => {
    const encoded = encodeFastSnapshot({
      oplog: new Uint8Array(),
      state: new Uint8Array(),
      shallowRootState: new Uint8Array(),
    });
    const document = decodeDocument(encoded);
    const withTrailing = encodeDocument(
      EncodeMode.FastSnapshot,
      new Uint8Array([...document.body, 0]),
    );
    expect(() => decodeFastSnapshot(withTrailing)).toThrow("trailing FastSnapshot bytes");
  });
});

describe("FastUpdates", () => {
  test("round trips length-prefixed postcard blocks", () => {
    const blocks = [new Uint8Array(), Uint8Array.of(1), new Uint8Array(300).fill(7)];
    expect(decodeFastUpdates(encodeFastUpdates(blocks))).toEqual(blocks);
    expect(encodeFastUpdates(blocks)).toEqual(
      encodeDocument(EncodeMode.FastUpdates, encodeFastUpdatesBody(blocks)),
    );
  });
});
