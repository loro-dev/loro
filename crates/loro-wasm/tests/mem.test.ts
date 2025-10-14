import { describe, expect, it } from "vitest";
import { LoroDoc } from "../bundler";

const MB = 1024 * 1024;
const ITERATIONS = 200;
const PAYLOAD_SIZE = 128 * 1024;

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

async function runWorkload(iterations: number, payloadSize: number) {
    const payload = "A".repeat(payloadSize);
    for (let i = 0; i < iterations; i++) {
        const doc = new LoroDoc();
        doc.getMap("map").set("key", payload);
        if ((i & 31) === 0) {
            await tick();
        }
    }
    await tick();
}

async function measureExternalDiff() {
    global.gc?.();
    const before = process.memoryUsage().external;
    await runWorkload(ITERATIONS, PAYLOAD_SIZE);
    global.gc?.();
    const after = process.memoryUsage().external;
    return after - before;
}

describe("memory", () => {
    it("should not grow external memory across runs", async () => {
        if (typeof global.gc !== "function") {
            console.warn("Skipping memory test because --expose-gc was not provided.");
            return;
        }

        // Warm the Wasm module so the high-water mark settles before assertions.
        await runWorkload(ITERATIONS, PAYLOAD_SIZE);

        const diffs: number[] = [];
        for (let run = 0; run < 3; run++) {
            const diff = await measureExternalDiff();
            diffs.push(diff);
        }

        console.log(
            "External memory diffs (MB):",
            diffs.map((diff) => (diff / MB).toFixed(2)).join(", ")
        );

        expect(diffs[0]).toBeLessThan(20 * MB);
        expect(Math.max(...diffs.slice(1))).toBeLessThan(5 * MB);
    }, 10000);
});
