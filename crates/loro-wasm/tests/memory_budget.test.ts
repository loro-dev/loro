import { describe, expect, expectTypeOf, it } from "vitest";
import {
  LoroDoc,
  UndoManager,
  type DocMemoryStats,
  type MemoryBudget,
  type MemoryLimitKind,
} from "../bundler/index";

function sumRamFields(stats: DocMemoryStats): number {
  return (
    stats.stateBytes +
    stats.valueCacheBytes +
    stats.kvBytes +
    stats.blockCacheBytes +
    stats.changeCacheBytes +
    stats.dagBytes +
    stats.arenaBytes +
    stats.pendingBytes +
    stats.historyCacheBytes +
    stats.diffCalcBytes +
    stats.undoBytes +
    stats.uncommittedTxnBytes
  );
}

function expectMemoryLimitError(
  fn: () => unknown,
  expectedKind: MemoryLimitKind,
): Error & {
  code: string;
  kind: MemoryLimitKind;
  usedBytes: number;
  limitBytes: number;
} {
  let caught: unknown;
  try {
    fn();
  } catch (err) {
    caught = err;
  }
  expect(caught).toBeInstanceOf(Error);
  const err = caught as Error & {
    code?: unknown;
    kind?: unknown;
    usedBytes?: unknown;
    limitBytes?: unknown;
  };
  expect(err.name).toBe("MemoryLimitExceededError");
  expect(err.code).toBe("MemoryLimitExceeded");
  expect(err.kind).toBe(expectedKind);
  expect(typeof err.usedBytes).toBe("number");
  expect(typeof err.limitBytes).toBe("number");
  expect((err.usedBytes as number) > (err.limitBytes as number)).toBe(true);
  return err as Error & {
    code: string;
    kind: MemoryLimitKind;
    usedBytes: number;
    limitBytes: number;
  };
}

describe("MemoryBudget & DocMemoryStats", () => {
  it("reports structured memoryStats and round-trips MemoryBudget", () => {
    const doc = new LoroDoc();
    doc.setPeerId("1");
    for (let i = 0; i < 8; i++) {
      doc.getMap(`m_${i}`).set("k", "payload_".repeat(32));
    }
    doc.getText("t").insert(0, "hello world ".repeat(20));
    doc.commit();

    const budget: MemoryBudget = {
      maxDocBytes: 1024 * 1024,
      highWatermarkBytes: 800 * 1024,
      targetWatermarkBytes: 600 * 1024,
      maxImportBlobBytes: 512 * 1024,
      maxPendingBytes: 256 * 1024,
      maxKvBlockCacheBytes: 64 * 1024,
      maxDecodedValueCacheBytes: 32 * 1024,
      maxUndoBytes: 128 * 1024,
    };
    doc.setMemoryBudget(budget);
    expect(doc.getMemoryBudget()).toStrictEqual(budget);

    const stats = doc.memoryStats();
    expectTypeOf(stats).toEqualTypeOf<DocMemoryStats>();
    expect(Object.keys(stats).sort()).toStrictEqual([
      "arenaBytes",
      "blockCacheBytes",
      "changeCacheBytes",
      "dagBytes",
      "diffCalcBytes",
      "historyCacheBytes",
      "kvBytes",
      "pendingBytes",
      "spilledDiskBytes",
      "stateBytes",
      "totalBytes",
      "uncommittedTxnBytes",
      "undoBytes",
      "valueCacheBytes",
    ]);
    expect(stats.spilledDiskBytes).toBe(0);
    expect(stats.totalBytes).toBe(sumRamFields(stats));
    expect(stats.stateBytes).toBeGreaterThan(0);

    const freed = doc.reclaimMemory();
    expect(typeof freed).toBe("number");
    const afterReclaim = doc.tryReclaimToBudget();
    expect(afterReclaim.totalBytes).toBe(sumRamFields(afterReclaim));
    expect(afterReclaim.totalBytes).toBeLessThanOrEqual(
      budget.maxDocBytes as number,
    );

    // tryReclaimToBudget rejects with MemoryLimitExceededError when maxDocBytes is tighter than minimum RAM.
    doc.setMemoryBudget({ maxDocBytes: 64 });
    expectMemoryLimitError(() => doc.tryReclaimToBudget(), "DocBudget");
  });

  it("enforces tryCommit atomically with active subscribe listener", () => {
    const doc = new LoroDoc();
    doc.setPeerId("1");
    const text = doc.getText("t");
    text.insert(0, "start");

    let eventCount = 0;
    const unsub = doc.subscribe((event) => {
      eventCount += event.events.length > 0 ? 1 : 0;
    });

    // Normal tryCommit must deliver event via decorateMethods without [LORO_INTERNAL_ERROR].
    doc.tryCommit({ origin: "init" });
    expect(eventCount).toBe(1);
    expect(text.toString()).toBe("start");

    const baseStats = doc.memoryStats();
    doc.setMemoryBudget({ maxDocBytes: baseStats.totalBytes + 1024 });

    // Oversized edit throws MemoryLimitExceededError, rolls back atomically, and emits 0 events.
    text.insert(5, "too_large_".repeat(128));
    expectMemoryLimitError(() => doc.tryCommit(), "DocBudget");
    expect(text.toString()).toBe("start");
    expect(doc.memoryStats().uncommittedTxnBytes).toBe(0);
    expect(eventCount).toBe(1);

    // Subsequent smaller edit commits cleanly and notifies subscriber.
    text.insert(5, "!");
    doc.tryCommit({ origin: "retry" });
    expect(text.toString()).toBe("start!");
    expect(eventCount).toBe(2);

    unsub();
  });

  it("enforces ImportBlob, PendingPool, and DocBudget across import, importBatch, importBatchDetailed, and importJsonUpdates", () => {
    const peer = new LoroDoc();
    peer.setPeerId("1");
    peer.getText("t").insert(0, "base ");
    peer.commit();
    const vv0 = peer.oplogVersion();
    const u0 = peer.export({ mode: "update" });

    peer.getText("t").insert(5, "small ");
    peer.commit();
    const vv1 = peer.oplogVersion();
    const u1Small = peer.export({ mode: "update", from: vv0 });

    for (let i = 0; i < 8; i++) {
      peer.getMap(`m_${i}`).set("big", "payload_".repeat(48));
      peer.commit();
    }
    const u2Big = peer.export({ mode: "update", from: vv1 });
    const jsonBig = peer.exportJsonUpdates(vv1, peer.oplogVersion());
    const blobLimit = Math.max(u0.length, u1Small.length);
    expect(u2Big.length).toBeGreaterThan(blobLimit);

    // 1) ImportBlob checks across import, importBatch, importBatchDetailed, importJsonUpdates
    const doc = new LoroDoc();
    doc.setPeerId("2");
    doc.setMemoryBudget({ maxImportBlobBytes: blobLimit });

    let events = 0;
    const unsub = doc.subscribe(() => {
      events += 1;
    });

    expectMemoryLimitError(() => doc.import(u2Big), "ImportBlob");
    expect(doc.getText("t").toString()).toBe("");
    expect(events).toBe(0);

    expectMemoryLimitError(() => doc.importBatch([u0, u2Big]), "ImportBlob");
    expect(doc.getText("t").toString()).toBe("");
    expect(events).toBe(0);

    const detail = doc.importBatchDetailed([u0, u2Big]);
    expect(detail.rollbackError).toBeNull();
    expect(detail.blobs[0].type).toBe("imported");
    expect(detail.blobs[1].type).toBe("failed");
    if (detail.blobs[1].type === "failed") {
      expect(detail.blobs[1].error).toContain("MemoryLimitExceeded");
    }
    expect(doc.getText("t").toString()).toBe("base ");
    expect(events).toBeGreaterThan(0);

    const eventsBeforeJson = events;
    expectMemoryLimitError(
      () => doc.importJsonUpdates(jsonBig),
      "ImportBlob",
    );
    expect(events).toBe(eventsBeforeJson);
    unsub();

    // 2) PendingPool rejection & mid-blob rollback
    const receiver = new LoroDoc();
    receiver.setPeerId("3");
    receiver.import(u1Small);
    const pendingBase = receiver.memoryStats().pendingBytes;
    expect(pendingBase).toBeGreaterThan(0);
    receiver.setMemoryBudget({ maxPendingBytes: pendingBase + 64 });
    expectMemoryLimitError(() => receiver.import(u2Big), "PendingPool");
    expect(receiver.memoryStats().pendingBytes).toBe(pendingBase);
    receiver.import(u0);
    expect(receiver.getText("t").toString()).toBe("base small ");
    expect(receiver.memoryStats().pendingBytes).toBe(0);

    // 3) DocBudget rollback in importBatchDetailed
    const batchDoc = new LoroDoc();
    batchDoc.setPeerId("4");
    batchDoc.getText("local").insert(0, "keep");
    batchDoc.commit();
    const batchBase = batchDoc.memoryStats().totalBytes;
    batchDoc.setMemoryBudget({ maxDocBytes: batchBase + u0.length + 1024 });

    let batchEvents = 0;
    const unsubBatch = batchDoc.subscribe(() => {
      batchEvents += 1;
    });

    const rbDetail = batchDoc.importBatchDetailed([u0, u2Big]);
    expect(rbDetail.rollbackError).not.toBeNull();
    expect(rbDetail.rollbackError).toContain("MemoryLimitExceeded");
    expect(rbDetail.blobs.every((b) => b.type === "rolledBack")).toBe(true);
    expect(batchDoc.isDetached()).toBe(false);
    expect(batchDoc.getText("local").toString()).toBe("keep");
    expect(batchDoc.getText("t").toString()).toBe("");
    expect(batchEvents).toBe(0);
    unsubBatch();
  });

  it("supports UndoManager maxUndoBytes config, setter, and usedBytes tracking", () => {
    const doc = new LoroDoc();
    doc.setPeerId("1");
    const text = doc.getText("t");

    const undo = new UndoManager(doc, {
      mergeInterval: 0,
      maxUndoBytes: 1024 * 1024,
    });

    for (let i = 0; i < 10; i++) {
      text.insert(text.length, `[s${i}:${"x".repeat(64)}]`);
      doc.commit();
    }
    const initialBytes = undo.usedBytes();
    expect(initialBytes).toBeGreaterThan(0);

    const tightCap = Math.floor(initialBytes / 3);
    undo.setMaxUndoBytes(tightCap);
    expect(undo.usedBytes()).toBeLessThanOrEqual(tightCap);
  });
});

it("applies a changed hard document quota after pending edits commit", () => {
  const doc = new LoroDoc();
  const text = doc.getText("text");
  text.insert(0, "before");
  doc.setMemoryBudget({ maxDocBytes: 1 });
  doc.tryCommit();
  expect(text.toString()).toBe("before");
  text.insert(6, "after");
  expect(() => doc.tryCommit()).toThrow();
  expect(text.toString()).toBe("before");
  doc.setMemoryBudget({});
  text.insert(6, "ok");
  doc.tryCommit();
  expect(text.toString()).toBe("beforeok");
});

it("forks inherit independent document memory policies", () => {
  const doc = new LoroDoc();
  doc.getText("text").insert(0, "before");
  doc.commit();
  const version = doc.frontiers();
  doc.setMemoryBudget({ maxDocBytes: 8 * 1024 * 1024, maxImportBlobBytes: 1 });
  for (const child of [doc.fork(), doc.forkAt(version)]) {
    expect(child.getMemoryBudget()).toEqual(doc.getMemoryBudget());
    expect(() => child.import(new Uint8Array(2))).toThrow();
    child.setMemoryBudget({});
    expect(doc.getMemoryBudget().maxImportBlobBytes).toBe(1);
    child.free();
  }
});
