import { describe, expect, test } from "vitest";

import { decodeChangeBlock, decodeFastUpdates, encodeFastUpdates } from "../src/codec";
import { LoroDoc } from "../src/index";

function sequentialUpdates(base = "") {
  const source = new LoroDoc();
  source.setPeerId(1);
  if (base !== "") source.getText("text").push(base);
  const snapshot = source.export({ mode: "snapshot" });
  const beforeA = source.oplogVersion();
  source.getText("text").push("A");
  const a = source.export({ mode: "update", from: beforeA });
  const beforeB = source.oplogVersion();
  source.getText("text").push("B");
  const b = source.export({ mode: "update", from: beforeB });
  return { source, snapshot, a, b };
}

function pendingChanges(bytes: Uint8Array) {
  return decodeFastUpdates(bytes).flatMap((block) => decodeChangeBlock(block).changes);
}

describe("exportPendingUpdates", () => {
  test("returns undefined without remote pending and does not commit local edits", () => {
    const doc = new LoroDoc();
    doc.setPeerId(9);
    expect(doc.exportPendingUpdates()).toBeUndefined();
    doc.getText("text").push("local");
    const uncommitted = doc.getUncommittedOpsAsJson();
    expect(doc.exportPendingUpdates()).toBeUndefined();
    expect(doc.getPendingTxnLength()).toBe(5);
    expect(doc.getUncommittedOpsAsJson()).toEqual(uncommitted);
    expect(doc.oplogVersion().toJSON()).toEqual(new Map());
    expect(doc.changeCount()).toBe(0);
  });

  test.each(["", "base|"])(
    "roundtrips missing dependencies beside a snapshot with base %j",
    (base) => {
      const { snapshot, a, b } = sequentialUpdates(base);
      const doc = LoroDoc.fromSnapshot(snapshot);
      const imported = doc.import(b);
      expect(imported.success.size).toBe(0);
      expect(imported.pending?.size).toBe(1);
      const version = doc.oplogVersion().toJSON();
      const frontiers = doc.oplogFrontiers();
      const state = doc.toJSON();
      const changes = doc.changeCount();

      const sidecar = doc.exportPendingUpdates()!;
      expect(sidecar).toBeInstanceOf(Uint8Array);
      expect(pendingChanges(sidecar)).toHaveLength(1);
      expect(doc.exportPendingUpdates()).toEqual(sidecar);
      expect(doc.oplogVersion().toJSON()).toEqual(version);
      expect(doc.oplogFrontiers()).toEqual(frontiers);
      expect(doc.toJSON()).toEqual(state);
      expect(doc.changeCount()).toBe(changes);

      const savedSnapshot = doc.export({ mode: "snapshot" });
      const ordinary = LoroDoc.fromSnapshot(savedSnapshot);
      ordinary.import(a);
      expect(ordinary.getText("text").toString()).toBe(`${base}A`);
      const updatesOnly = new LoroDoc();
      updatesOnly.import(doc.export({ mode: "update" }));
      updatesOnly.import(a);
      expect(updatesOnly.getText("text").toString()).toBe(`${base}A`);

      let restored = LoroDoc.fromSnapshot(savedSnapshot);
      for (let save = 0; save < 2; save++) {
        expect(restored.import(sidecar).pending).toEqual(imported.pending);
        expect(restored.import(sidecar).pending).toEqual(imported.pending);
        expect(restored.getText("text").toString()).toBe(base);
        const savedPending = restored.exportPendingUpdates()!;
        restored = LoroDoc.fromSnapshot(restored.export({ mode: "snapshot" }));
        expect(restored.import(savedPending).pending).toEqual(imported.pending);
      }
      expect(restored.import(a).pending).toBeNull();
      expect(restored.getText("text").toString()).toBe(`${base}AB`);
      expect(restored.exportPendingUpdates()).toBeUndefined();
      expect(restored.import(sidecar).pending).toBeNull();
      expect(restored.getText("text").toString()).toBe(`${base}AB`);

      expect(doc.import(a).pending).toBeNull();
      expect(doc.getText("text").toString()).toBe(`${base}AB`);
      expect(doc.exportPendingUpdates()).toBeUndefined();
    },
  );

  test("exports remote pending independently of an uncommitted local transaction", () => {
    const { a, b } = sequentialUpdates();
    const doc = new LoroDoc();
    doc.setPeerId(9);
    doc.import(b);
    doc.getMap("map").set("local", true);
    const uncommitted = doc.getUncommittedOpsAsJson();
    let commits = 0;
    doc.subscribeLocalUpdates(() => commits++);

    const sidecar = doc.exportPendingUpdates()!;
    expect(doc.getPendingTxnLength()).toBe(1);
    expect(doc.getUncommittedOpsAsJson()).toEqual(uncommitted);
    expect(doc.oplogVersion().toJSON()).toEqual(new Map());
    expect(commits).toBe(0);
    const restored = new LoroDoc();
    expect(restored.import(sidecar).pending?.size).toBe(1);
    restored.import(a);
    expect(restored.toJSON()).toEqual({ text: "AB" });
    expect(doc.getMap("map").get("local")).toBe(true);
  });

  test("excludes operations already applied from a partially successful import", () => {
    const { a, b } = sequentialUpdates();
    const independent = new LoroDoc();
    independent.setPeerId(2);
    independent.getMap("map").set("applied", true);
    const mixed = encodeFastUpdates([
      ...decodeFastUpdates(independent.export({ mode: "update" })),
      ...decodeFastUpdates(b),
    ]);
    const doc = new LoroDoc();
    const imported = doc.import(mixed);
    expect(imported.success).toEqual(new Map([["2", { start: 0, end: 1 }]]));
    expect(imported.pending).toEqual(new Map([["1", { start: 1, end: 2 }]]));
    const pending = doc.exportPendingUpdates()!;
    expect(pendingChanges(pending).map((change) => change.id)).toEqual([
      { peer: 1n, counter: 1 },
    ]);
    const restored = LoroDoc.fromSnapshot(doc.export({ mode: "snapshot" }));
    restored.import(pending);
    restored.import(a);
    expect(restored.toJSON()).toEqual({ text: "AB", map: { applied: true } });
    expect(restored.exportPendingUpdates()).toBeUndefined();
  });

  test("keeps multiple peers and slots through partial unpending and repeated saves", () => {
    const { source, snapshot, a, b } = sequentialUpdates("base|");
    const dependent = LoroDoc.fromSnapshot(source.export({ mode: "snapshot" }));
    dependent.setPeerId(2);
    dependent.getMap("map").set("cross-peer", true);
    const c = dependent.export({ mode: "update", from: source.oplogVersion() });

    const independent = new LoroDoc();
    independent.setPeerId(3);
    independent.getMap("map").set("first", true);
    const first = independent.export({ mode: "update" });
    const beforeLast = independent.oplogVersion();
    independent.getMap("map").set("last", true);
    const last = independent.export({ mode: "update", from: beforeLast });

    let doc = LoroDoc.fromSnapshot(snapshot);
    doc.import(c);
    doc.import(b);
    expect(doc.import(last).pending?.size).toBe(3);
    const pending = doc.exportPendingUpdates()!;
    doc = LoroDoc.fromSnapshot(doc.export({ mode: "snapshot" }));
    expect(doc.import(pending).pending?.size).toBe(3);
    expect(doc.import(a).pending).toEqual(new Map([["3", { start: 1, end: 2 }]]));
    expect(doc.toJSON()).toEqual({ text: "base|AB", map: { "cross-peer": true } });
    const remaining = doc.exportPendingUpdates()!;
    expect(pendingChanges(remaining).map((change) => change.id)).toEqual([
      { peer: 3n, counter: 1 },
    ]);
    doc = LoroDoc.fromSnapshot(doc.export({ mode: "snapshot" }));
    doc.import(remaining);
    doc.import(remaining);
    expect(doc.import(first).pending).toBeNull();
    expect(doc.getMap("map").toJSON()).toEqual({
      "cross-peer": true,
      first: true,
      last: true,
    });
    expect(doc.exportPendingUpdates()).toBeUndefined();
  });

  test("exports only pending tails and deduplicates overlapping input ranges", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    source.getText("text").push("ABCDE");
    source.commit();
    const range = (counter: number, len: number) =>
      source.export({
        mode: "updates-in-range",
        spans: [{ id: { peer: "1", counter }, len }],
      });
    const doc = new LoroDoc();
    doc.import(range(0, 1));
    doc.import(range(2, 2));
    doc.import(range(3, 2));
    doc.import(range(2, 2));
    const pending = doc.exportPendingUpdates()!;
    const operations = pendingChanges(pending).flatMap((change) =>
      change.operations.flatMap((operation) =>
        Array.from(
          { length: operation.length },
          (_, offset) => operation.counter + offset,
        ),
      ),
    );
    expect(operations).toEqual([2, 3, 4]);
    const restored = LoroDoc.fromSnapshot(doc.export({ mode: "snapshot" }));
    expect(restored.import(pending).pending).toEqual(
      new Map([["1", { start: 2, end: 5 }]]),
    );
    restored.import(range(1, 1));
    expect(restored.getText("text").toString()).toBe("ABCDE");
    expect(restored.exportPendingUpdates()).toBeUndefined();
    doc.import(range(1, 2));
    expect(doc.getText("text").toString()).toBe("ABCDE");
    expect(doc.exportPendingUpdates()).toBeUndefined();
  });
});
