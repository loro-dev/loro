import { afterEach, describe, expect, expectTypeOf, it } from "vitest";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  globalMemoryStats,
  getGlobalMemoryBudget,
  LoroDoc,
  reclaimGlobalMemory,
  setGlobalMemoryBudget,
  tryReclaimGlobalToBudget,
  type GlobalMemoryBudget,
  type GlobalMemoryStats,
  type MemoryLimitKind,
} from "../bundler/index";

function expectGlobalLimitError(
  fn: () => void,
  expectedKind: MemoryLimitKind = "GlobalBudget",
) {
  let caught: any;
  try {
    fn();
  } catch (e) {
    caught = e;
  }
  expect(caught).toBeInstanceOf(Error);
  expect(caught.name).toBe("MemoryLimitExceededError");
  expect(caught.code).toBe("MemoryLimitExceeded");
  expect(caught.kind).toBe(expectedKind);
  expect(typeof caught.usedBytes).toBe("number");
  expect(typeof caught.limitBytes).toBe("number");
  expect(caught.usedBytes).toBeGreaterThan(caught.limitBytes);
  return caught;
}

afterEach(() => {
  setGlobalMemoryBudget({});
});

describe("Global WASM MemoryBudget & GlobalMemoryStats", () => {
  it("tracks real WASM heap, linear memory pages, liveDocCount, and safe 64-bit budget values >= 4 GiB", () => {
    const budget: GlobalMemoryBudget = {
      maxHeapBytes: 3_500_000_000,
      maxWasmLinearBytes: 4 * 1024 * 1024 * 1024, // 4 GiB (> u32::MAX)
      highWatermarkBytes: 2_800_000_000,
      targetWatermarkBytes: 2_000_000_000,
      maxImportBlobBytes: 64 * 1024 * 1024,
      maxKvBlockCacheBytes: 8 * 1024 * 1024,
    };
    setGlobalMemoryBudget(budget);
    expect(getGlobalMemoryBudget()).toStrictEqual(budget);

    const before = globalMemoryStats();
    expectTypeOf(before).toEqualTypeOf<GlobalMemoryStats>();
    expect(before.allocatedHeapBytes).toBeGreaterThan(0);
    expect(before.peakAllocatedHeapBytes).toBeGreaterThanOrEqual(
      before.allocatedHeapBytes,
    );
    expect(before.wasmLinearMemoryBytes).toBeGreaterThanOrEqual(
      before.allocatedHeapBytes,
    );
    expect(before.wasmLinearMemoryBytes % 65536).toBe(0);

    const doc = new LoroDoc();
    const text = doc.getText("body");
    text.insert(0, "hello ".repeat(4000));
    doc.commit();

    const during = globalMemoryStats();
    expect(during.liveDocCount).toBeGreaterThanOrEqual(before.liveDocCount + 1);
    expect(during.allocatedHeapBytes).toBeGreaterThan(before.allocatedHeapBytes);

    text.free();
    doc.free();
    const after = globalMemoryStats();
    expect(after.liveDocCount).toBe(before.liveDocCount);
    expect(after.peakAllocatedHeapBytes).toBeGreaterThanOrEqual(
      during.allocatedHeapBytes,
    );
  });

  it("reclaims idle docs across the WASM instance and preserves JSON state", () => {
    const docs: LoroDoc[] = [];
    const expected: unknown[] = [];
    for (let i = 0; i < 6; i++) {
      const src = new LoroDoc();
      src.setPeerId(i + 1);
      for (let k = 0; k < 20; k++) {
        const m = src.getMap(`m_${k}`);
        m.set("payload", `d${i}_${k}_` + "x".repeat(512));
        m.free();
      }
      src.commit();
      const snap = src.export({ mode: "snapshot" });
      src.free();

      const d = new LoroDoc();
      d.setPeerId(100 + i);
      d.import(snap);
      for (let k = 0; k < 20; k++) {
        const m = d.getMap(`m_${k}`);
        m.set("local", k);
        m.free();
      }
      d.commit();
      expected.push(d.toJSON());
      docs.push(d);
    }

    const before = globalMemoryStats();
    // Set non-triggering watermarks first so explicit `reclaimGlobalMemory()` exercises
    // full idle-doc eviction directly.
    setGlobalMemoryBudget({
      maxHeapBytes: before.allocatedHeapBytes * 2,
      highWatermarkBytes: before.allocatedHeapBytes * 2,
      targetWatermarkBytes: Math.floor(before.allocatedHeapBytes * 0.45),
    });

    const freed = reclaimGlobalMemory();
    expect(freed).toBeGreaterThan(0);
    const after = globalMemoryStats();
    expect(after.allocatedHeapBytes).toBeLessThan(before.allocatedHeapBytes);

    for (let i = 0; i < docs.length; i++) {
      expect(docs[i].toJSON()).toStrictEqual(expected[i]);
      docs[i].free();
    }
  });

  it("rejects oversized Uint8Array before copying into WASM heap and rolls back oversized tryCommit/importBatch", () => {
    const idle = new LoroDoc();
    idle.setPeerId(1);
    const idleText = idle.getText("t");
    idleText.insert(0, "p".repeat(16 * 1024));
    idleText.free();
    idle.commit();

    const active = new LoroDoc();
    active.setPeerId(2);
    const activeRoot = active.getMap("root");
    activeRoot.set("k", "v");
    active.commit();

    let events = 0;
    const unsub = active.subscribe(() => {
      events += 1;
    });

    const pseudoRandom = (seed: number, len: number) => {
      let s = (seed + 0x9e3779b9) >>> 0;
      const chars = new Array<string>(len);
      for (let i = 0; i < len; i++) {
        s = (Math.imul(1664525, s) + 1013904223) >>> 0;
        chars[i] = String.fromCharCode(97 + ((s >>> 24) % 26));
      }
      return chars.join("");
    };

    const s1 = new LoroDoc();
    s1.setPeerId(101);
    const s1t = s1.getText("t1");
    s1t.insert(0, pseudoRandom(1, 24 * 1024));
    s1t.free();
    s1.commit();
    const u1 = s1.export({ mode: "update" });
    s1.free();

    const s2 = new LoroDoc();
    s2.setPeerId(102);
    const s2t = s2.getText("t2");
    s2t.insert(0, pseudoRandom(2, 24 * 1024));
    s2t.free();
    s2.commit();
    const u2 = s2.export({ mode: "update" });
    s2.free();

    reclaimGlobalMemory();
    const base = globalMemoryStats();
    const linearBeforeGiant = base.wasmLinearMemoryBytes;

    setGlobalMemoryBudget({
      maxHeapBytes: base.allocatedHeapBytes + 256 * 1024,
      highWatermarkBytes: base.allocatedHeapBytes + 128 * 1024,
      targetWatermarkBytes: base.allocatedHeapBytes,
    });

    // 1) Giant 8 MiB Uint8Array is rejected BEFORE allocating/growing WASM linear memory!
    const giant = new Uint8Array(8 * 1024 * 1024);
    expectGlobalLimitError(() => active.import(giant), "GlobalBudget");
    expect(globalMemoryStats().wasmLinearMemoryBytes).toBe(linearBeforeGiant);
    expect(events).toBe(0);

    // 1b) With only maxHeapBytes set and headroom between 1x and 3x of an ASCII string
    // (100 KiB ASCII with 256 KiB headroom), LoroText.insert must NOT be falsely rejected
    // by a 3x worst-case UTF-8 preflight estimate.
    const fitDoc = new LoroDoc();
    fitDoc.setPeerId(77);
    const fitText = fitDoc.getText("fit");
    fitText.insert(0, "A".repeat(100 * 1024));
    expect(fitText.length).toBe(100 * 1024);
    fitText.free();
    fitDoc.free();

    // 2) Oversized local tryCommit fails with GlobalBudget and rolls back atomically.
    const snapBefore = active.toJSON();
    const bigText = active.getText("big");
    bigText.insert(0, "Z".repeat(512 * 1024));
    bigText.free();
    expectGlobalLimitError(() => active.tryCommit(), "GlobalBudget");
    expect(active.toJSON()).toStrictEqual(snapBefore);
    expect(events).toBe(0);

    // 3) Very tight maxHeapBytes rejects each update pre-malloc in importBatch / importBatchDetailed.
    reclaimGlobalMemory();
    const tightBase = globalMemoryStats();
    setGlobalMemoryBudget({
      maxHeapBytes: tightBase.allocatedHeapBytes + 4 * 1024,
      highWatermarkBytes: tightBase.allocatedHeapBytes + 2 * 1024,
      targetWatermarkBytes: tightBase.allocatedHeapBytes,
    });
    expectGlobalLimitError(() => active.importBatch([u1, u2]), "GlobalBudget");
    expect(active.toJSON()).toStrictEqual(snapBefore);

    const perBlobRejectDetail = active.importBatchDetailed([u1, u2]);
    expect(perBlobRejectDetail.rollbackError).toBeNull();
    expect(perBlobRejectDetail.blobs[0].type).toBe("failed");
    expect(perBlobRejectDetail.blobs[1].type).toBe("failed");
    if (perBlobRejectDetail.blobs[0].type === "failed") {
      expect(perBlobRejectDetail.blobs[0].error).toContain("MemoryLimitExceeded");
    }
    expect(perBlobRejectDetail.status.success.size).toBe(0);
    expect(events).toBe(0);

    // When each blob passes individual preflight (~24 KiB each) and loading, but the resulting
    // materialized state pushes heap past maxHeapBytes, `importBatchDetailed` rolls both back.
    reclaimGlobalMemory();
    const batchBase = globalMemoryStats();
    const batchBudgetMargin =
      u1.byteLength + Math.floor(u2.byteLength * 1.1) + 4 * 1024;
    setGlobalMemoryBudget({
      maxHeapBytes: batchBase.allocatedHeapBytes + batchBudgetMargin,
      highWatermarkBytes:
        batchBase.allocatedHeapBytes + Math.floor(batchBudgetMargin * 0.75),
      targetWatermarkBytes: batchBase.allocatedHeapBytes,
    });
    const detail = active.importBatchDetailed([u1, u2]);
    expect(detail.rollbackError).toContain("MemoryLimitExceeded");
    expect(detail.rollbackError).toContain("global_budget");
    expect(detail.blobs.every((b) => b.type === "rolledBack")).toBe(true);
    expect(detail.status.success.size).toBe(0);
    expect(active.toJSON()).toStrictEqual(snapBefore);
    expect(events).toBe(0);

    // 4) Freeing idle recovers headroom and allows commits again.
    idle.free();
    setGlobalMemoryBudget({
      maxHeapBytes: batchBase.allocatedHeapBytes + 64 * 1024,
    });
    tryReclaimGlobalToBudget();
    activeRoot.set("ok", true);
    active.tryCommit();
    expect(events).toBe(1);
    expect((active.toJSON() as any).root.ok).toBe(true);

    // Tight ceiling makes tryReclaimGlobalToBudget fail cleanly.
    setGlobalMemoryBudget({ maxHeapBytes: 128 });
    expectGlobalLimitError(() => tryReclaimGlobalToBudget(), "GlobalBudget");
    unsub();
    activeRoot.free();
    active.free();
  });

  it("prevents OOM WASM trap inside a 64 MiB Durable-Object-style V8 memory cap", (ctx) => {
    // A file URL: Node rejects an absolute Windows path as a module specifier.
    const nodejsEntry = pathToFileURL(
      path.resolve(
        path.dirname(fileURLToPath(import.meta.url)),
        "../nodejs/index.js",
      ),
    ).href;
    const probe = spawnSync(
      process.execPath,
      ["--wasm-max-mem-pages=1024", "-e", ""],
      { encoding: "utf8" },
    );
    if (probe.error || String(probe.stderr).includes("bad option")) {
      ctx.skip();
    }

    const script = `
      import {
        LoroDoc,
        setGlobalMemoryBudget,
        globalMemoryStats,
      } from ${JSON.stringify(nodejsEntry)};

      // Durable Object style guard inside a 64 MiB WASM hard ceiling (1024 pages).
      setGlobalMemoryBudget({
        maxHeapBytes: 24 * 1024 * 1024,
        maxWasmLinearBytes: 48 * 1024 * 1024,
        highWatermarkBytes: 16 * 1024 * 1024,
        targetWatermarkBytes: 10 * 1024 * 1024,
        maxKvBlockCacheBytes: 2 * 1024 * 1024,
      });

      const docs = [];
      for (let i = 0; i < 20; i++) {
        const d = new LoroDoc();
        d.setPeerId(i + 1);
        for (let k = 0; k < 25; k++) {
          const m = d.getMap("m_" + k);
          m.set("v", "chunk_" + i + "_" + k + "_" + "a".repeat(2048));
          m.free();
        }
        d.tryCommit();
        docs.push(d);
      }

      // 1) Oversized 30 MiB Uint8Array import is rejected pre-malloc (0 WASM bytes copied).
      let importRejectedKind = "";
      try {
        docs[0].import(new Uint8Array(30 * 1024 * 1024));
      } catch (e) {
        importRejectedKind = String(e.name) + ":" + String(e.kind);
      }

      // 2) Catastrophic 26 MiB JS string LoroText.insert is rejected pre-malloc before hitting 64 MiB V8 OOM trap.
      let hugeStringRejectedKind = "";
      const hugeText = docs[0].getText("catastrophic");
      try {
        hugeText.insert(0, "X".repeat(26 * 1024 * 1024));
      } catch (e) {
        hugeStringRejectedKind = String(e.name) + ":" + String(e.kind);
      }
      hugeText.free();

      // 3) Moderate 14 MiB edit grows past maxHeapBytes (24 MiB) / maxWasmLinearBytes (48 MiB) on tryCommit(),
      //    throws GlobalBudget, and rolls back atomically WITHOUT trapping V8's 64 MiB limit.
      let commitRejectedKind = "";
      const overText = docs[0].getText("overflow");
      overText.insert(0, "Y".repeat(14 * 1024 * 1024));
      overText.free();
      try {
        docs[0].tryCommit();
      } catch (e) {
        commitRejectedKind = String(e.name) + ":" + String(e.kind);
      }

      // 4) Isolate is NOT trapped and docs[0] rolled back cleanly; existing docs stay usable
      //    within existing freed dlmalloc pages (without growing linear memory toward 64 MiB).
      const rolledBack = (docs[0].toJSON()).overflow === undefined;
      const lastMap = docs[docs.length - 1].getMap("m_0");
      lastMap.set("alive", true);
      lastMap.free();
      docs[docs.length - 1].tryCommit();
      const aliveVal = (docs[docs.length - 1].toJSON()).m_0.alive;
      const stats = globalMemoryStats();
      console.log("RESULT:" + JSON.stringify({
        importRejectedKind,
        hugeStringRejectedKind,
        commitRejectedKind,
        rolledBack,
        aliveVal,
        liveDocCount: stats.liveDocCount,
        wasmLinearMemoryBytes: stats.wasmLinearMemoryBytes,
        allocatedHeapBytes: stats.allocatedHeapBytes,
      }));
    `;

    const res = spawnSync(
      process.execPath,
      ["--wasm-max-mem-pages=1024", "--input-type=module", "-e", script],
      { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
    );
    if (res.error) {
      throw res.error;
    }
    expect(res.status, `stderr: ${res.stderr}\nstdout: ${res.stdout}`).toBe(0);
    const line = res.stdout
      .split("\n")
      .find((l) => l.startsWith("RESULT:"));
    expect(line).toBeDefined();
    const parsed = JSON.parse(line!.slice("RESULT:".length));
    expect(parsed.importRejectedKind).toBe(
      "MemoryLimitExceededError:GlobalBudget",
    );
    expect(parsed.hugeStringRejectedKind).toBe(
      "MemoryLimitExceededError:GlobalBudget",
    );
    expect(parsed.commitRejectedKind).toBe(
      "MemoryLimitExceededError:GlobalBudget",
    );
    expect(parsed.rolledBack).toBe(true);
    expect(parsed.aliveVal).toBe(true);
    expect(parsed.liveDocCount).toBe(20);
    expect(parsed.wasmLinearMemoryBytes).toBeLessThan(64 * 1024 * 1024);
    expect(parsed.allocatedHeapBytes).toBeLessThan(24 * 1024 * 1024);
  }, 60_000);
});

it("does not repeatedly demote idle state after WASM pages stay high but heap is free", () => {
  const transient = new LoroDoc();
  const transientText = transient.getText("large");
  transientText.insert(0, "x".repeat(8 * 1024 * 1024));
  transient.commit();
  transientText.free();
  transient.free();
  const stats = globalMemoryStats();
  const linearLimit = Math.ceil(stats.wasmLinearMemoryBytes * 1.1);
  expect(stats.allocatedHeapBytes).toBeLessThan(linearLimit * 0.7);
  setGlobalMemoryBudget({ maxWasmLinearBytes: linearLimit });
  const idle = new LoroDoc();
  for (let i = 0; i < 16; i++) {
    const item = idle.getMap(`idle-${i}`);
    item.set("v", i);
    item.free();
  }
  idle.commit();
  const before = idle.memoryStats().stateBytes;
  const active = new LoroDoc();
  const map = active.getMap("active");
  for (let i = 0; i < 20; i++) {
    map.set("v", i);
    active.tryCommit();
  }
  expect(idle.memoryStats().stateBytes).toBe(before);
  const last = idle.getMap("idle-15");
  expect(last.get("v")).toBe(15);
  last.free();
  map.free();
  active.free();
  idle.free();
});

it("tryReclaimGlobalToBudget accepts grown pages once the heap fits, and reports the pages", () => {
  const transient = new LoroDoc();
  const transientText = transient.getText("large");
  transientText.insert(0, "x".repeat(8 * 1024 * 1024));
  transient.commit();
  transientText.free();
  transient.free();
  const stats = globalMemoryStats();
  // Below the pages already allocated, but with room for the live heap.
  const linearLimit = stats.wasmLinearMemoryBytes - 65536;
  expect(stats.allocatedHeapBytes).toBeLessThan(linearLimit * 0.7);
  setGlobalMemoryBudget({ maxWasmLinearBytes: linearLimit });
  try {
    expect(globalMemoryStats().wasmLinearMemoryExceeded).toBe(true);
    // Used to throw forever once the pages grew, while tryCommit succeeded.
    const after = tryReclaimGlobalToBudget();
    expect(after.wasmLinearMemoryExceeded).toBe(true);
    const doc = new LoroDoc();
    doc.getMap("m").set("v", 1);
    doc.tryCommit();
    doc.free();
  } finally {
    setGlobalMemoryBudget({});
  }
  expect(globalMemoryStats().wasmLinearMemoryExceeded).toBe(false);
});
