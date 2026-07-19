import { describe, expect, test } from "vitest";
import { bytesToHex, decodeLz4Frame, encodeLz4FrameRaw, hexToBytes, } from "../src/codec/index";
describe("LZ4 frame", () => {
    test("decodes a compressed overlapping match", () => {
        const frame = hexToBytes("04224d18604082040000001061010000000000");
        expect(new TextDecoder().decode(decodeLz4Frame(frame))).toBe("aaaaa");
    });
    test.each([0, 1, 65536, 65537, 300000])("round trips %i canonical frame bytes", (length) => {
        const input = new Uint8Array(length);
        for (let index = 0; index < input.length; index += 1) {
            input[index] = index & 0xff;
        }
        expect(decodeLz4Frame(encodeLz4FrameRaw(input), {
            requireCanonicalProfile: true,
        })).toEqual(input);
    });
    test("encodes an overlapping match with the required trailing literals", () => {
        const input = new TextEncoder().encode("aaaaaaaaaaaaaaaaaa");
        const encoded = encodeLz4FrameRaw(input);
        expect(bytesToHex(encoded)).toBe("04224d186040820a0000001861010050616161616100000000");
        expect(decodeLz4Frame(encoded)).toEqual(input);
    });
    test("compresses highly repetitive blocks", () => {
        const input = new Uint8Array(256 * 1024).fill(0x5a);
        const encoded = encodeLz4FrameRaw(input);
        expect(readFirstBlockInfo(encoded) & 2147483648).toBe(0);
        expect(encoded.length).toBeLessThan(input.length / 100);
        expect(decodeLz4Frame(encoded)).toEqual(input);
    });
    test("stores incompressible blocks without expansion", () => {
        const input = deterministicNoise(32 * 1024);
        const encoded = encodeLz4FrameRaw(input);
        expect(readFirstBlockInfo(encoded) & 2147483648).not.toBe(0);
        expect(encoded.length).toBe(input.length + 15);
        expect(decodeLz4Frame(encoded)).toEqual(input);
    });
    test("round trips mixed noisy and repeated ranges", () => {
        for (const [caseIndex, length] of [
            13, 14, 15, 31, 255, 256, 4095, 65535, 65536, 65537,
        ].entries()) {
            const input = deterministicNoise(length, 305419896 + caseIndex);
            for (let start = 97; start < input.length; start += 211) {
                const copyLength = Math.min(80, input.length - start);
                for (let index = 0; index < copyLength; index += 1) {
                    input[start + index] = input[index % 37];
                }
            }
            expect(decodeLz4Frame(encodeLz4FrameRaw(input))).toEqual(input);
        }
    });
});
function readFirstBlockInfo(frame) {
    return new DataView(frame.buffer, frame.byteOffset + 7, 4).getUint32(0, true);
}
function deterministicNoise(length, seed = 305419896) {
    const output = new Uint8Array(length);
    let state = seed;
    for (let index = 0; index < output.length; index += 1) {
        state ^= state << 13;
        state ^= state >>> 17;
        state ^= state << 5;
        output[index] = state >>> 24;
    }
    return output;
}
