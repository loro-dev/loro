import { describe, expect, test } from "vitest";

import {
  ContainerType,
  decodeChangeBlock,
  decodeFastUpdates,
  encodeChangeBlock,
  encodeFastUpdates,
  unknownContainerType,
} from "../src/codec";
import { LoroDoc } from "../src/index";
import { loadRustReference } from "./support/rust-reference";

const rust = loadRustReference();

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

  test("restores post-root pending updates beside a shallow snapshot", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    source.getText("text").push("old|");
    source.commit();
    source.getText("text").push("root|");
    source.commit();
    const root = source.oplogFrontiers();
    const rootVersion = source.oplogVersion();
    const shallow = source.export({ mode: "shallow-snapshot", frontiers: root });
    source.getText("text").push("A");
    const a = source.export({ mode: "update", from: rootVersion });
    const afterA = source.oplogVersion();
    source.getText("text").push("B");
    const b = source.export({ mode: "update", from: afterA });
    const doc = LoroDoc.fromSnapshot(shallow);
    expect(doc.import(b).pending?.size).toBe(1);
    const sidecar = doc.exportPendingUpdates()!;
    const snapshot = doc.export({ mode: "snapshot" });
    const ordinary = LoroDoc.fromSnapshot(snapshot);
    ordinary.import(a);
    expect(ordinary.getText("text").toString()).toBe("old|root|A");
    const restored = LoroDoc.fromSnapshot(snapshot);
    for (const target of [doc, restored]) {
      expect(target.isShallow()).toBe(true);
      expect(target.shallowSinceFrontiers()).toEqual(root);
      expect(target.import(sidecar).pending?.size).toBe(1);
      expect(target.getText("text").toString()).toBe("old|root|");
      expect(target.import(a).pending).toBeNull();
      expect(target.toJSON()).toEqual(source.toJSON());
      expect(target.oplogVersion().toJSON()).toEqual(source.oplogVersion().toJSON());
      expect(target.shallowSinceFrontiers()).toEqual(root);
      expect(target.exportPendingUpdates()).toBeUndefined();
      target.import(sidecar);
      expect(target.getText("text").toString()).toBe("old|root|AB");
      expect(target.exportPendingUpdates()).toBeUndefined();
    }
  });

  test("rejects a sidecar whose dependencies precede the shallow root", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    source.getMap("map").set("before", 1);
    const before = source.export({ mode: "snapshot" });
    const beforeVersion = source.oplogVersion();
    source.getMap("map").set("root", 2);
    source.commit();
    const root = source.oplogFrontiers();
    const shallow = source.export({ mode: "shallow-snapshot", frontiers: root });
    const fork = LoroDoc.fromSnapshot(before);
    fork.setPeerId(2);
    fork.getMap("map").set("concurrent", true);
    const concurrent = fork.export({ mode: "update", from: beforeVersion });
    const pendingDoc = new LoroDoc();
    expect(pendingDoc.import(concurrent).pending?.size).toBe(1);
    const sidecar = pendingDoc.exportPendingUpdates()!;
    const doc = LoroDoc.fromSnapshot(shallow);
    const state = doc.toJSON();
    const version = doc.oplogVersion().toJSON();
    expect(() => doc.import(sidecar)).toThrow(
      "cannot import updates that depend on an outdated version",
    );
    expect(doc.toJSON()).toEqual(state);
    expect(doc.oplogVersion().toJSON()).toEqual(version);
    expect(doc.shallowSinceFrontiers()).toEqual(root);
    expect(doc.exportPendingUpdates()).toBeUndefined();
    expect(pendingDoc.exportPendingUpdates()).toBeInstanceOf(Uint8Array);
    pendingDoc.import(before);
    expect(pendingDoc.toJSON()).toEqual(fork.toJSON());
    expect(pendingDoc.exportPendingUpdates()).toBeUndefined();
    source.getMap("map").set("after", 3);
    doc.import(source.export({ mode: "update", from: doc.oplogVersion() }));
    expect(doc.toJSON()).toEqual(source.toJSON());
  });

  test.skipIf(rust === undefined).each([64, 128, 256, 512])(
    "compacts shared Rust block keys for %i pending changes without altering values",
    (count) => {
      const source = new rust!.LoroDoc();
      source.setPeerId("1");
      source.getMap("root").set("missing", true);
      const missing = source.export({ mode: "update" });
      const from = source.oplogVersion();
      for (let index = 0; index < count; index++) {
        source.getMap("root").set(`key${index}`, {
          index,
          nested: { label: `value${index}`, values: [{ id: index }] },
        });
        source.commit({ message: `change${index}` });
      }
      const input = source.export({ mode: "update", from });
      const doc = new LoroDoc();
      expect(doc.import(input).pending?.size).toBe(1);
      doc.import(input);
      const snapshot = doc.export({ mode: "snapshot" });
      doc.getMap("draft").set("local", true);
      const local = doc.getUncommittedOpsAsJson();
      const sidecar = doc.exportPendingUpdates()!;
      expect(sidecar.length).toBeLessThan(count * 384 + 1024);
      const blocks = decodeFastUpdates(sidecar).map(decodeChangeBlock);
      expect(blocks).toHaveLength(count);
      for (const block of blocks) expect(block.keys.length).toBeLessThanOrEqual(8);
      expect(doc.exportPendingUpdates()).toEqual(sidecar);
      expect(doc.getUncommittedOpsAsJson()).toEqual(local);
      expect(doc.oplogVersion().toJSON()).toEqual(new Map());
      for (const Target of [LoroDoc, rust!.LoroDoc]) {
        const restored = new Target();
        restored.import(snapshot);
        expect(restored.import(sidecar).pending?.size).toBe(1);
        restored.import(missing);
        expect(restored.toJSON()).toEqual(source.toJSON());
        expect(restored.exportPendingUpdates()).toBeUndefined();
        restored.import(sidecar);
        expect(restored.toJSON()).toEqual(source.toJSON());
      }
    },
  );

  test.skipIf(rust === undefined)(
    "preserves opaque future history while detached through both runtimes",
    () => {
      const source = new rust!.LoroDoc();
      source.setPeerId("1");
      source.getMap("root").set("missing", true);
      const missing = source.export({ mode: "update" });
      const keys = ["unused", "future", "property", "style", "nested"];
      const input = encodeFastUpdates([
        encodeChangeBlock({
          peers: [1n],
          keys,
          containers: [],
          positions: [],
          changes: [
            {
              id: { peer: 1n, counter: 1 },
              timestamp: 0n,
              lamport: 1,
              dependencies: [{ peer: 1n, counter: 0 }],
              message: undefined,
              operations: [
                {
                  container: {
                    kind: "root",
                    name: "future",
                    containerType: unknownContainerType(42),
                  },
                  counter: 1,
                  length: 1,
                  content: {
                    type: "future",
                    property: 2,
                    value: {
                      type: "mark-start",
                      info: 0,
                      length: 1n,
                      keyIndex: 3n,
                      value: {
                        type: "map",
                        value: [[4n, { type: "string", value: "preserved" }]],
                      },
                    },
                  },
                },
              ],
            },
          ],
        }),
      ]);
      const doc = new LoroDoc();
      doc.detach();
      doc.import(input);
      const sidecar = doc.exportPendingUpdates()!;
      const block = decodeChangeBlock(decodeFastUpdates(sidecar)[0]!);
      expect(block.keys).toEqual(keys);
      expect(block.changes[0]!.operations[0]!.content).toEqual(
        decodeChangeBlock(decodeFastUpdates(input)[0]!).changes[0]!.operations[0]!
          .content,
      );
      expect(doc.exportPendingUpdates()).toEqual(sidecar);
      // Pure TypeScript does not support applying Unknown containers yet.
      const restoredTs = new LoroDoc();
      restoredTs.detach();
      expect(restoredTs.import(sidecar).pending?.size).toBe(1);
      expect(() => restoredTs.import(missing)).toThrow("unsupported container type 42");
      expect(restoredTs.exportPendingUpdates()).toEqual(sidecar);
      const restored = new rust!.LoroDoc();
      restored.detach();
      expect(restored.import(sidecar).pending?.size).toBe(1);
      restored.import(missing);
      expect(restored.oplogVersion().toJSON().get("1")).toBe(2);
      expect(restored.isDetached()).toBe(true);
      expect(restored.exportPendingUpdates()).toBeUndefined();
      const output = decodeFastUpdates(restored.export({ mode: "update" }))
        .flatMap((bytes) => {
          const decoded = decodeChangeBlock(bytes);
          return decoded.changes.flatMap((change) =>
            change.operations.map((operation) => ({ operation, keys: decoded.keys })),
          );
        })
        .find(
          ({ operation }) => operation.container.containerType !== ContainerType.Map,
        )!;
      expect(output.operation.content.type).toBe("future");
      if (output.operation.content.type !== "future")
        throw new Error("future operation missing");
      expect(output.operation.content.property).toBe(2);
      const value = output.operation.content.value;
      expect(value.type).toBe("mark-start");
      if (value.type !== "mark-start") throw new Error("future value missing");
      expect(output.keys[Number(value.keyIndex)]).toBe("style");
      expect(value.value.type).toBe("map");
      if (value.value.type !== "map") throw new Error("nested value missing");
      expect(output.keys[Number(value.value.value[0]![0])]).toBe("nested");
      expect(value.value.value[0]![1]).toEqual({ type: "string", value: "preserved" });
    },
  );

  test.skipIf(rust === undefined)(
    "keeps original keys for an unrecognized Counter property",
    () => {
      const source = new rust!.LoroDoc();
      source.setPeerId("1");
      source.getMap("root").set("missing", true);
      const missing = source.export({ mode: "update" });
      const keys = ["unused", "counter", "future-key"];
      const input = encodeFastUpdates([
        encodeChangeBlock({
          peers: [1n],
          keys,
          containers: [],
          positions: [],
          changes: [
            {
              id: { peer: 1n, counter: 1 },
              timestamp: 0n,
              lamport: 1,
              dependencies: [{ peer: 1n, counter: 0 }],
              message: undefined,
              operations: [
                {
                  container: {
                    kind: "root",
                    name: "counter",
                    containerType: ContainerType.Counter,
                  },
                  counter: 1,
                  length: 1,
                  content: {
                    type: "future",
                    property: 7,
                    value: { type: "double", value: 0.5 },
                  },
                },
              ],
            },
          ],
        }),
      ]);
      const doc = new LoroDoc();
      doc.import(input);
      const sidecar = doc.exportPendingUpdates()!;
      const block = decodeChangeBlock(decodeFastUpdates(sidecar)[0]!);
      expect(block.keys).toEqual(keys);
      expect(block.changes[0]!.operations[0]!.content).toEqual({
        type: "future",
        property: 7,
        value: { type: "double", value: 0.5 },
      });
      expect(doc.exportPendingUpdates()).toEqual(sidecar);
      for (const Target of [LoroDoc, rust!.LoroDoc]) {
        const restored = new Target();
        expect(restored.import(sidecar).pending?.size).toBe(1);
        restored.import(missing);
        expect(restored.toJSON()).toEqual({ root: { missing: true }, counter: 0.5 });
        expect(restored.exportPendingUpdates()).toBeUndefined();
      }
    },
  );
});
