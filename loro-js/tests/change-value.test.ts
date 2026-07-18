import { describe, expect, test } from "vitest";

import { bytesToHex, decodeChangeValue, encodeChangeValue } from "../src/codec/index";
import type { ChangeValue } from "../src/codec/index";

const bytes = (hex: string): Uint8Array =>
  Uint8Array.from(hex.match(/../g) ?? [], (byte) => Number.parseInt(byte, 16));

describe("change value codec", () => {
  test.each<[string, ChangeValue]>([
    ["00", { type: "null" }],
    ["01", { type: "bool", value: true }],
    ["02", { type: "bool", value: false }],
    ["037f", { type: "i64", value: -1n }],
    ["0a7f", { type: "delta-int", value: -1 }],
    ["05026869", { type: "string", value: "hi" }],
    ["0603010203", { type: "binary", value: Uint8Array.of(1, 2, 3) }],
    ["07ac02", { type: "container-index", value: 300n }],
    ["08", { type: "delete-once" }],
    ["09", { type: "delete-sequence" }],
    ["043ff0000000000000", { type: "double", value: 1 }],
    [
      "0b07020301050178",
      {
        type: "loro-value",
        value: {
          type: "list",
          value: [
            { type: "i64", value: 1n },
            { type: "string", value: "x" },
          ],
        },
      },
    ],
    [
      "0b08010000",
      {
        type: "loro-value",
        value: { type: "map", value: [[0n, { type: "null" }]] },
      },
    ],
    [
      "0c84010001",
      {
        type: "mark-start",
        info: 0x84,
        length: 1n,
        keyIndex: 0n,
        value: { type: "bool", value: true },
      },
    ],
    [
      "0d010102",
      {
        type: "tree-move",
        targetIndex: 1n,
        parentIsNull: true,
        position: 2n,
        parentIndex: undefined,
      },
    ],
    [
      "0d01000203",
      {
        type: "tree-move",
        targetIndex: 1n,
        parentIsNull: false,
        position: 2n,
        parentIndex: 3n,
      },
    ],
    ["0e010203", { type: "list-move", from: 1n, fromPeerIndex: 2n, lamport: 3n }],
    [
      "0f072a0301",
      {
        type: "list-set",
        peerIndex: 7n,
        lamport: 42,
        value: { type: "i64", value: 1n },
      },
    ],
    [
      "1001020301",
      {
        type: "raw-tree-move",
        subjectPeerIndex: 1n,
        subjectCounter: 2,
        positionIndex: 3n,
        parentIsNull: true,
        parentPeerIndex: 0n,
        parentCounter: 0,
      },
    ],
    [
      "10010203000405",
      {
        type: "raw-tree-move",
        subjectPeerIndex: 1n,
        subjectCounter: 2,
        positionIndex: 3n,
        parentIsNull: false,
        parentPeerIndex: 4n,
        parentCounter: 5,
      },
    ],
    ["910158", { type: "future", tag: 0x91, data: Uint8Array.of(0x58) }],
  ])("matches the known bytes %s", (hex, value) => {
    expect(decodeChangeValue(bytes(hex))).toEqual(value);
    expect(bytesToHex(encodeChangeValue(value))).toBe(hex);
  });
});
