import { describe, expect, test } from "vitest";

import { decodeLz4Frame, encodeLz4FrameRaw, hexToBytes } from "../src/codec/index";

describe("LZ4 frame", () => {
  test("decodes a compressed overlapping match", () => {
    const frame = hexToBytes("04224d18604082040000001061010000000000");
    expect(new TextDecoder().decode(decodeLz4Frame(frame))).toBe("aaaaa");
  });

  test.each([0, 1, 65_536, 65_537, 300_000])("round trips %i raw bytes", (length) => {
    const input = new Uint8Array(length);
    for (let index = 0; index < input.length; index += 1) {
      input[index] = index & 0xff;
    }
    expect(
      decodeLz4Frame(encodeLz4FrameRaw(input), { requireCanonicalProfile: true }),
    ).toEqual(input);
  });
});
