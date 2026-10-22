import { describe, expect, expectTypeOf, it } from "vitest";
import { LoroDoc } from "../bundler/index";

function pendingBytes(doc: LoroDoc): Uint8Array {
  const bytes = doc.exportPendingUpdates();
  expectTypeOf(bytes).toEqualTypeOf<Uint8Array | undefined>();
  expect(bytes).toBeInstanceOf(Uint8Array);
  if (bytes === undefined) throw new Error("Expected pending remote updates");
  return bytes;
}

function append(doc: LoroDoc, value: string): Uint8Array {
  const from = doc.oplogVersion();
  const text = doc.getText("text");
  text.insert(text.toString().length, value);
  doc.commit();
  return doc.export({ mode: "update", from });
}

function linearUpdates(base = "") {
  const source = new LoroDoc();
  source.setPeerId("1");
  if (base) append(source, base);
  const snapshot = source.export({ mode: "snapshot" });
  const a = append(source, "A");
  const b = append(source, "B");
  return { source, snapshot, a, b };
}

function restore(snapshot: Uint8Array, pending: Uint8Array): LoroDoc {
  const restored = new LoroDoc();
  restored.import(snapshot);
  const status = restored.import(pending);
  expect(status.success.size).toBe(0);
  expect(status.pending?.size).toBeGreaterThan(0);
  return restored;
}

describe("exportPendingUpdates", () => {
  it("returns undefined without remote pending updates, including local uncommitted operations", () => {
    const doc = new LoroDoc();
    expect(doc.exportPendingUpdates()).toBeUndefined();
    doc.getText("text").insert(0, "local");
    expect(doc.getPendingTxnLength()).toBe(5);
    expect(doc.exportPendingUpdates()).toBeUndefined();
    expect(doc.getPendingTxnLength()).toBe(5);
    doc.commit();
    expect(doc.exportPendingUpdates()).toBeUndefined();
  });

  it.each(["", "base|"])(
    "restores out-of-order updates alongside a snapshot with base %j",
    (base) => {
      const { snapshot, a, b } = linearUpdates(base);
      const doc = new LoroDoc();
      doc.import(snapshot);
      const status = doc.import(b);
      expect(status.success.size).toBe(0);
      expect(status.pending?.get("1")).toEqual({
        start: base.length + 1,
        end: base.length + 2,
      });

      let events = 0;
      const unsubscribe = doc.subscribe(() => events++);
      const state = doc.toJSON();
      const version = doc.version().toJSON();
      const oplogVersion = doc.oplogVersion().toJSON();
      const frontiers = doc.frontiers();
      const oplogFrontiers = doc.oplogFrontiers();
      const pending = pendingBytes(doc);
      pendingBytes(doc);
      expect(doc.toJSON()).toEqual(state);
      expect(doc.version().toJSON()).toEqual(version);
      expect(doc.oplogVersion().toJSON()).toEqual(oplogVersion);
      expect(doc.frontiers()).toEqual(frontiers);
      expect(doc.oplogFrontiers()).toEqual(oplogFrontiers);
      expect(events).toBe(0);
      unsubscribe();

      const savedSnapshot = doc.export({ mode: "snapshot" });
      const snapshotOnly = new LoroDoc();
      snapshotOnly.import(savedSnapshot);
      snapshotOnly.import(a);
      expect(snapshotOnly.getText("text").toString()).toBe(`${base}A`);
      const updatesOnly = new LoroDoc();
      updatesOnly.import(doc.export({ mode: "update" }));
      updatesOnly.import(a);
      expect(updatesOnly.getText("text").toString()).toBe(`${base}A`);

      const restored = restore(savedSnapshot, pending);
      expect(restored.getText("text").toString()).toBe(base);
      pendingBytes(restored);
      restored.import(a);
      doc.import(a);
      for (const complete of [doc, restored]) {
        expect(complete.getText("text").toString()).toBe(`${base}AB`);
        expect(complete.exportPendingUpdates()).toBeUndefined();
      }
    },
  );

  it("survives repeated saves, restores, and duplicate imports", () => {
    const { snapshot, a, b } = linearUpdates("base|");
    let doc = new LoroDoc();
    doc.import(snapshot);
    doc.import(b);
    const firstPending = pendingBytes(doc);
    for (let round = 0; round < 3; round++) {
      const savedSnapshot = doc.export({ mode: "snapshot" });
      const savedPending = pendingBytes(doc);
      doc = restore(savedSnapshot, savedPending);
      doc.import(savedPending);
      doc.import(b);
      expect(doc.getText("text").toString()).toBe("base|");
      pendingBytes(doc);
    }
    doc.import(a);
    doc.import(a);
    doc.import(firstPending);
    doc.import(b);
    expect(doc.getText("text").toString()).toBe("base|AB");
    expect(doc.exportPendingUpdates()).toBeUndefined();
  });

  it("persists only the remainder as multiple peers gradually receive their dependencies", () => {
    const { source, snapshot, a, b } = linearUpdates();
    const afterB = source.export({ mode: "snapshot" });
    const c = append(source, "C");
    const afterC = source.export({ mode: "snapshot" });
    const d = append(source, "D");

    const secondPeer = new LoroDoc();
    secondPeer.setPeerId("2");
    secondPeer.import(afterB);
    const secondPeerStart = secondPeer.oplogVersion();
    secondPeer.getMap("map").set("x", 1);
    const x = secondPeer.export({ mode: "update", from: secondPeerStart });

    const thirdPeer = new LoroDoc();
    thirdPeer.setPeerId("3");
    thirdPeer.import(afterC);
    const thirdPeerStart = thirdPeer.oplogVersion();
    thirdPeer.getMap("map").set("y", 2);
    const y = thirdPeer.export({ mode: "update", from: thirdPeerStart });

    let doc = new LoroDoc();
    doc.import(snapshot);
    for (const update of [d, x, b, y, b, d]) doc.import(update);
    doc = restore(doc.export({ mode: "snapshot" }), pendingBytes(doc));
    const allPending = pendingBytes(doc);
    doc.import(a);
    expect(doc.getText("text").toString()).toBe("AB");
    expect(doc.getMap("map").toJSON()).toEqual({ x: 1 });

    const remaining = pendingBytes(doc);
    const remainingOnly = new LoroDoc();
    const remainingStatus = remainingOnly.import(remaining);
    expect(remainingStatus.success.size).toBe(0);
    expect(remainingStatus.pending).toEqual(
      new Map([
        ["1", { start: 3, end: 4 }],
        ["3", { start: 0, end: 1 }],
      ]),
    );

    doc = restore(doc.export({ mode: "snapshot" }), remaining);
    doc.import(c);
    doc.import(allPending);
    doc.import(c);
    expect(doc.getText("text").toString()).toBe("ABCD");
    expect(doc.getMap("map").toJSON()).toEqual({ x: 1, y: 2 });
    expect(doc.exportPendingUpdates()).toBeUndefined();
  });

  it("handles partially applied batches and overlapping pending ranges", () => {
    const source = new LoroDoc();
    source.setPeerId("1");
    append(source, "abcdefghij");
    const range = (start: number, len: number) => ({
      id: { peer: "1" as const, counter: start },
      len,
    });
    const prefix = source.export({
      mode: "updates-in-range",
      spans: [range(0, 2)],
    });
    const initialPending = source.export({
      mode: "updates-in-range",
      spans: [range(4, 4)],
    });
    const overlapping = source.export({
      mode: "updates-in-range",
      spans: [range(6, 4)],
    });
    const missing = source.export({
      mode: "updates-in-range",
      spans: [range(2, 2)],
    });

    const doc = new LoroDoc();
    const status = doc.importBatch([prefix, initialPending]);
    expect(status.success.get("1")).toEqual({ start: 0, end: 2 });
    expect(status.pending?.get("1")).toEqual({ start: 4, end: 8 });
    doc.import(overlapping);
    doc.importBatch([prefix, initialPending]);
    doc.import(overlapping);
    expect(doc.getText("text").toString()).toBe("ab");

    const pending = pendingBytes(doc);
    const pendingOnly = new LoroDoc();
    const pendingStatus = pendingOnly.import(pending);
    expect(pendingStatus.success.size).toBe(0);
    expect(pendingStatus.pending?.get("1")).toEqual({ start: 4, end: 10 });
    const restored = restore(doc.export({ mode: "snapshot" }), pending);
    restored.import(missing);
    restored.import(pending);
    expect(restored.getText("text").toString()).toBe("abcdefghij");
    expect(restored.exportPendingUpdates()).toBeUndefined();
  });

  it("does not include, consume, or commit a local transaction while exporting remote pending updates", () => {
    const { snapshot, a, b } = linearUpdates("base|");
    const doc = new LoroDoc();
    doc.setPeerId("2");
    doc.import(snapshot);
    doc.import(b);
    let commits = 0;
    let events = 0;
    const unsubscribeCommit = doc.subscribePreCommit(() => commits++);
    const unsubscribe = doc.subscribe(() => events++);
    doc.getMap("local").set("uncommitted", true);
    const txnLength = doc.getPendingTxnLength();
    expect(txnLength).toBeGreaterThan(0);
    const txn = doc.getUncommittedOpsAsJson();
    const oplogVersion = doc.oplogVersion().toJSON();
    const frontiers = doc.oplogFrontiers();

    const pending = pendingBytes(doc);
    pendingBytes(doc);
    expect(doc.getPendingTxnLength()).toBe(txnLength);
    expect(doc.getUncommittedOpsAsJson()).toEqual(txn);
    expect(doc.oplogVersion().toJSON()).toEqual(oplogVersion);
    expect(doc.oplogFrontiers()).toEqual(frontiers);
    expect(commits).toBe(0);
    expect(events).toBe(0);

    const restored = restore(snapshot, pending);
    restored.import(a);
    expect(restored.toJSON()).toEqual({ text: "base|AB" });
    expect(restored.exportPendingUpdates()).toBeUndefined();
    doc.commit();
    expect(commits).toBe(1);
    expect(events).toBe(1);
    unsubscribeCommit();
    unsubscribe();
    doc.import(a);
    expect(doc.toJSON()).toEqual({
      text: "base|AB",
      local: { uncommitted: true },
    });
    expect(doc.exportPendingUpdates()).toBeUndefined();
  });
});
