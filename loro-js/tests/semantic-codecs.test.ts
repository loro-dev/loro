import { describe, expect, test } from "vitest";

import {
  ContainerType,
  bytesToHex,
  decodeChangeBlockKey,
  decodeContainerId,
  decodePostcardContainerId,
  decodePostcardFrontiers,
  decodePostcardOptionalContainerId,
  decodePostcardValue,
  decodePostcardVersionVector,
  encodeChangeBlockKey,
  encodeContainerId,
  encodePostcardContainerId,
  encodePostcardFrontiers,
  encodePostcardOptionalContainerId,
  encodePostcardValue,
  encodePostcardVersionVector,
} from "../src/codec/index";
import type { ContainerId, EncodedLoroValue, Id } from "../src/codec/index";

describe("IDs and container IDs", () => {
  test("matches the Rust ChangeStore key layout", () => {
    const id: Id = { peer: 0x0102_0304_0506_0708n, counter: -1 };
    const encoded = encodeChangeBlockKey(id);
    expect(bytesToHex(encoded)).toBe("0102030405060708ffffffff");
    expect(decodeChangeBlockKey(encoded)).toEqual(id);
  });

  test.each<[ContainerId, string]>([
    [{ kind: "root", name: "root", containerType: ContainerType.Map }, "8004726f6f74"],
    [
      {
        kind: "normal",
        peer: 7n,
        counter: 42,
        containerType: ContainerType.List,
      },
      "0107000000000000002a000000",
    ],
  ])("matches the raw ContainerID layout", (id, expected) => {
    const encoded = encodeContainerId(id);
    expect(bytesToHex(encoded)).toBe(expected);
    expect(decodeContainerId(encoded)).toEqual(id);
  });

  test.each<[ContainerId | undefined, string]>([
    [undefined, "00"],
    [
      { kind: "root", name: "root", containerType: ContainerType.Map },
      "010004726f6f7401",
    ],
    [{ kind: "root", name: "t", containerType: ContainerType.Text }, "0100017400"],
    [
      {
        kind: "normal",
        peer: 7n,
        counter: 42,
        containerType: ContainerType.List,
      },
      "0101075402",
    ],
  ])("matches Rust postcard Option<ContainerID>", (id, expected) => {
    const encoded = encodePostcardOptionalContainerId(id);
    expect(bytesToHex(encoded)).toBe(expected);
    expect(decodePostcardOptionalContainerId(encoded)).toEqual(id);
  });

  test("round trips postcard ContainerID without the option wrapper", () => {
    const id: ContainerId = {
      kind: "normal",
      peer: 0xffff_ffff_ffff_ffffn,
      counter: -0x8000_0000,
      containerType: ContainerType.Tree,
    };
    expect(decodePostcardContainerId(encodePostcardContainerId(id))).toEqual(id);
  });
});

describe("VersionVector and Frontiers", () => {
  test("preserves VersionVector order", () => {
    const version = [
      { peer: 7n, counter: 42 },
      { peer: 1n, counter: -1 },
    ];
    expect(decodePostcardVersionVector(encodePostcardVersionVector(version))).toEqual(
      version,
    );
  });

  test("canonically sorts Frontiers by peer then counter", () => {
    const frontiers = [
      { peer: 7n, counter: 42 },
      { peer: 1n, counter: 0 },
      { peer: 7n, counter: 41 },
      { peer: 1n, counter: -1 },
    ];
    expect(decodePostcardFrontiers(encodePostcardFrontiers(frontiers))).toEqual([
      { peer: 1n, counter: -1 },
      { peer: 1n, counter: 0 },
      { peer: 7n, counter: 41 },
      { peer: 7n, counter: 42 },
    ]);
  });
});

describe("postcard LoroValue", () => {
  const values: EncodedLoroValue[] = [
    { type: "null" },
    { type: "bool", value: false },
    { type: "bool", value: true },
    { type: "double", value: -0 },
    { type: "i64", value: -1n },
    { type: "string", value: "hi 😀" },
    { type: "binary", value: Uint8Array.of(1, 2) },
    {
      type: "list",
      value: [{ type: "null" }, { type: "i64", value: 1n }],
    },
    {
      type: "map",
      value: [
        ["a", { type: "null" }],
        ["b", { type: "i64", value: 2n }],
      ],
    },
    {
      type: "container",
      value: { kind: "root", name: "x", containerType: ContainerType.Text },
    },
  ];

  test.each(values)("round trips $type", (value) => {
    const decoded = decodePostcardValue(encodePostcardValue(value));
    expect(decoded).toEqual(value);
    expect(
      value.type !== "double" || Object.is((decoded as { value: number }).value, -0),
    ).toBe(true);
  });
});
