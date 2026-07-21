import { describe, expect, test } from "vitest";

import {
  LoroDecodeError,
  SstableReader,
  decodeSstable,
  encodeSstable,
} from "../src/codec/index";

describe("SSTable", () => {
  test("uses zero bytes for an empty KV store", () => {
    expect(encodeSstable([])).toEqual(new Uint8Array());
    expect(decodeSstable(new Uint8Array())).toEqual([]);
  });

  test.each(["none", "auto", "lz4"] as const)(
    "round trips with %s compression",
    (compression) => {
      const entries = [
        { key: Uint8Array.of(1), value: new Uint8Array() },
        { key: Uint8Array.of(1, 2), value: Uint8Array.of(4, 5) },
        { key: Uint8Array.of(2), value: new Uint8Array(5000).fill(9) },
      ];
      expect(decodeSstable(encodeSstable(entries, { compression }))).toEqual(entries);
    },
  );

  test("sorts keys and rejects duplicates", () => {
    const encoded = encodeSstable([
      { key: Uint8Array.of(2), value: Uint8Array.of(2) },
      { key: Uint8Array.of(1), value: Uint8Array.of(1) },
    ]);
    expect(decodeSstable(encoded).map((entry) => entry.key[0])).toEqual([1, 2]);
    expect(() =>
      encodeSstable([
        { key: Uint8Array.of(1), value: new Uint8Array() },
        { key: Uint8Array.of(1), value: new Uint8Array() },
      ]),
    ).toThrow("unique");
  });

  test("checks the document-level block checksum", () => {
    const encoded = encodeSstable([{ key: Uint8Array.of(1), value: Uint8Array.of(2) }]);
    const corrupted = encoded.slice();
    corrupted[6] = corrupted[6]! ^ 1;
    expect(() => decodeSstable(corrupted)).toThrow(LoroDecodeError);
  });

  test("looks up and rewrites entries one block at a time", () => {
    const source = Array.from({ length: 12 }, (_, index) => ({
      key: Uint8Array.of(index + 1),
      value: new Uint8Array(12).fill(index + 1),
    }));
    const table = new SstableReader(
      encodeSstable(source, { blockSize: 32, compression: "lz4" }),
    );

    expect(table.get(Uint8Array.of(7))).toEqual(new Uint8Array(12).fill(7));
    expect(table.get(Uint8Array.of(99))).toBeUndefined();

    const rewritten = table.rewrite(
      [
        { key: Uint8Array.of(0), value: Uint8Array.of(100) },
        { key: Uint8Array.of(2), value: Uint8Array.of(20) },
        { key: Uint8Array.of(3), value: undefined },
        { key: Uint8Array.of(13), value: Uint8Array.of(130) },
      ],
      { blockSize: 32, compression: "auto" },
    );
    expect(
      decodeSstable(rewritten).map(({ key, value }) => [key[0], [...value]]),
    ).toEqual([
      [0, [100]],
      [1, Array(12).fill(1)],
      [2, [20]],
      ...source.slice(3).map(({ key, value }) => [key[0], [...value]]),
      [13, [130]],
    ]);
  });
});
