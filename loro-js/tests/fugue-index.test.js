import { describe, expect, test, vi } from "vitest";
import { LoroText } from "../src/index";
import { SequenceIndex } from "../src/runtime/sequence-index";
describe("Fugue origin index", () => {
    test("does not probe every descendant in a concurrent text run", () => {
        const probeCount = (length) => {
            const text = new LoroText();
            text._insertFugue(0, "a".repeat(length), { peer: 1n, counter: 0 }, 0, new Map());
            const atPhysical = vi.spyOn(SequenceIndex.prototype, "atPhysicalRaw");
            text._insertFugue(0, "b", { peer: 2n, counter: 0 }, 0, new Map());
            const probes = atPhysical.mock.calls.length;
            atPhysical.mockRestore();
            expect(text.toString()).toBe(`${"a".repeat(length)}b`);
            return probes;
        };
        const shortRunProbes = probeCount(128);
        const longRunProbes = probeCount(16384);
        expect(longRunProbes).toBeLessThanOrEqual(shortRunProbes + 2);
        expect(longRunProbes).toBeLessThan(8);
    });
});
