import { describe, expect, it } from "vitest";
import { LoroDoc, LoroList, LoroMap, LoroText } from "../bundler";

const MB = 1024 * 1024;

/**
 * Builds a document shaped like a chat history: a root list of turn maps,
 * each with an items list of maps carrying nested maps/texts. With
 * turns=1300 and itemsPerTurn=60 this yields ~100k containers.
 */
function buildSnapshot(turns: number, itemsPerTurn: number): Uint8Array {
    const TEXT = "The quick brown fox jumps over the lazy dog. ".repeat(4);
    const doc = new LoroDoc();
    const history = doc.getList("history");
    for (let t = 0; t < turns; t++) {
        const turn = history.pushContainer(new LoroMap());
        turn.set("id", `turn-${t}`);
        turn.set("role", t % 2 === 0 ? "user" : "assistant");
        const items = turn.setContainer("items", new LoroList());
        const count = t % 2 === 0 ? 1 : itemsPerTurn;
        for (let i = 0; i < count; i++) {
            const item = items.pushContainer(new LoroMap());
            if (i % 4 === 3) {
                item.set("type", "text");
                item.setContainer("text", new LoroText()).insert(0, `${TEXT} ${t}/${i}`);
            } else {
                item.set("type", "tool_call");
                item.set("toolCallId", `tc-${t}-${i}`);
                const raw = item.setContainer("rawInput", new LoroMap());
                raw.set("command", "pnpm test");
                raw.set("cwd", "/repo");
                item.setContainer("title", new LoroText()).insert(0, TEXT);
            }
        }
    }
    doc.commit();
    const snapshot = doc.export({ mode: "snapshot" });
    doc.free();
    return snapshot;
}

type ContainerHandle = { kind(): string };

const isContainer = (v: unknown): v is ContainerHandle =>
    !!v && typeof v === "object" && typeof (v as ContainerHandle).kind === "function";

/** loro-mirror-style traversal: keys()/get() per map, get(i) per list, toJSON() per text. */
function walk(container: ContainerHandle): unknown {
    const kind = container.kind();
    if (kind === "Map") {
        const map = container as unknown as { keys(): string[]; get(k: string): unknown };
        const out: Record<string, unknown> = {};
        for (const key of map.keys()) {
            const value = map.get(key);
            out[key] = isContainer(value) ? walk(value) : value;
        }
        return out;
    }
    if (kind === "List" || kind === "MovableList") {
        const list = container as unknown as { length: number; get(i: number): unknown };
        const out: unknown[] = [];
        for (let i = 0; i < list.length; i++) {
            const value = list.get(i);
            out.push(isContainer(value) ? walk(value) : value);
        }
        return out;
    }
    return (container as unknown as { toJSON(): unknown }).toJSON();
}

describe("container handle walk memory", () => {
    // Regression test for https://github.com/loro-dev/loro/issues/1092:
    // reading an imported document container by container through JS handles
    // used to pin ~4 KB of wasm linear memory per container until doc.free().
    it("keeps wasm memory bounded while walking every container", () => {
        if (typeof global.gc !== "function") {
            console.warn("Skipping memory test because --expose-gc was not provided.");
            return;
        }

        const snapshot = buildSnapshot(1300, 60);

        // Bulk-read baseline: toJSON() reads the same state without retaining
        // per-container decoded values.
        const docA = new LoroDoc();
        docA.import(snapshot);
        global.gc();
        const beforeToJson = process.memoryUsage().external;
        docA.toJSON();
        global.gc();
        const toJsonDelta = process.memoryUsage().external - beforeToJson;
        docA.free();

        // Handle walk over every container of an identical document.
        const docB = new LoroDoc();
        docB.import(snapshot);
        global.gc();
        const beforeWalk = process.memoryUsage().external;
        walk(docB.getList("history") as unknown as ContainerHandle);
        global.gc();
        const walkDelta = process.memoryUsage().external - beforeWalk;
        docB.free();

        console.log(
            `external deltas: toJSON=${(toJsonDelta / MB).toFixed(1)}MB walk=${(walkDelta / MB).toFixed(1)}MB`,
        );

        // With the leak, the walk retained ~4 KB per container (~400 MB at
        // this size). It must now stay within a small multiple of the bulk
        // read, modulo allocator noise.
        const allowance = Math.max(3 * Math.max(toJsonDelta, 0), 48 * MB);
        expect(walkDelta).toBeLessThan(allowance);
    }, 120_000);
});
