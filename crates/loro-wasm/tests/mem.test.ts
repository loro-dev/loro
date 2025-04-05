import { describe, expect, it } from "vitest";
import { LoroDoc } from "../bundler";

describe("memory", () => {
    it("should not leak memory", async () => {
        global.gc && global.gc();
        const before = process.memoryUsage();
        const largeString = "A".repeat(1024 * 1024);
        for (let i = 0; i < 1000; i++) {
            const doc = new LoroDoc();
            doc.getMap("map").set("key", largeString);
            await new Promise((resolve) => setTimeout(resolve, 0));
        }

        global.gc && global.gc();
        const memoryUsage = process.memoryUsage();
        // console.log("mem", memoryUsage);
        console.log(
            "Memory diff (MB):",
            `external: ${((memoryUsage.external - before.external) / 1024 / 1024).toFixed(2)}`,
            `rss: ${((memoryUsage.rss - before.rss) / 1024 / 1024).toFixed(2)}`
        );
        expect(memoryUsage.external - before.external).toBeLessThan(100 * 1024 * 1024);
        expect(memoryUsage.rss - before.rss).toBeLessThan(100 * 1024 * 1024);
    }, 10000)
})
