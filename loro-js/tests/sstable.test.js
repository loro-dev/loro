import { describe, expect, test } from "vitest";
import { LoroDecodeError, decodeSstable, encodeSstable } from "../src/codec/index";
describe("SSTable", () => {
    test("uses zero bytes for an empty KV store", () => {
        expect(encodeSstable([])).toEqual(new Uint8Array());
        expect(decodeSstable(new Uint8Array())).toEqual([]);
    });
    test.each(["none", "auto", "lz4"])("round trips with %s compression", (compression) => {
        const entries = [
            { key: Uint8Array.of(1), value: new Uint8Array() },
            { key: Uint8Array.of(1, 2), value: Uint8Array.of(4, 5) },
            { key: Uint8Array.of(2), value: new Uint8Array(5000).fill(9) },
        ];
        expect(decodeSstable(encodeSstable(entries, { compression }))).toEqual(entries);
    });
    test("sorts keys and rejects duplicates", () => {
        const encoded = encodeSstable([
            { key: Uint8Array.of(2), value: Uint8Array.of(2) },
            { key: Uint8Array.of(1), value: Uint8Array.of(1) },
        ]);
        expect(decodeSstable(encoded).map((entry) => entry.key[0])).toEqual([1, 2]);
        expect(() => encodeSstable([
            { key: Uint8Array.of(1), value: new Uint8Array() },
            { key: Uint8Array.of(1), value: new Uint8Array() },
        ])).toThrow("unique");
    });
    test("checks the document-level block checksum", () => {
        const encoded = encodeSstable([{ key: Uint8Array.of(1), value: Uint8Array.of(2) }]);
        const corrupted = encoded.slice();
        corrupted[6] = corrupted[6] ^ 1;
        expect(() => decodeSstable(corrupted)).toThrow(LoroDecodeError);
    });
});
