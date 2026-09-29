import { describe, expect, test } from "vitest";

import {
  LoroDoc,
  LoroMap,
  LoroText,
  UndoManager,
  VersionVector,
  type JsonSchema,
  type LoroEventBatch,
} from "../src/index";

/**
 * A failed import or importBatch leaves no trace: value, version, frontiers,
 * pending changes, events and undo are as before, and the document stays
 * usable. Rust's contract: context/import-batch-atomicity.md.
 */

function doc(peer: number): LoroDoc {
  const created = new LoroDoc();
  created.setPeerId(peer);
  return created;
}

interface Observed {
  readonly value: unknown;
  readonly version: [string, number][];
  readonly frontiers: unknown;
  readonly changes: number;
  readonly ops: number;
  readonly shallow: string;
}

function observe(target: LoroDoc): Observed {
  return {
    value: target.toJSON(),
    version: [...target.oplogVersion().toJSON()].sort(([left], [right]) =>
      left.localeCompare(right),
    ),
    frontiers: target.oplogFrontiers(),
    changes: target.changeCount(),
    ops: target.opCount(),
    shallow: JSON.stringify(target.getShallowValue()),
  };
}

/** Peer 1's list [a, b, c] and a later change by peer 1 that moves `a`. */
function base(): { readonly source: LoroDoc; readonly update: Uint8Array } {
  const source = doc(1);
  const list = source.getMovableList("list");
  for (const value of ["a", "b", "c"]) list.push(value);
  source.commit();
  return { source, update: source.export({ mode: "update" }) };
}

/** A JSON change whose move op names `from`, which may be out of range. */
function forgedMove(source: LoroDoc, from: number): JsonSchema {
  const version = source.oplogVersion();
  source.getMovableList("list").move(0, 2);
  source.commit();
  const json = source.exportJsonUpdates(version);
  const content = json.changes[0]!.ops[0]!.content as { from: number };
  content.from = from;
  return json;
}

function assertUsable(target: LoroDoc): void {
  const list = target.getMovableList("list");
  const length = list.length;
  list.push("local");
  target.commit();
  expect(list.length).toBe(length + 1);
  const remote = new LoroDoc();
  remote.setPeerId(9);
  remote.import(target.export({ mode: "snapshot" }));
  remote.getMovableList("list").push("remote");
  remote.commit();
  target.import(remote.export({ mode: "update", from: target.oplogVersion() }));
  expect(target.toJSON()).toEqual(remote.toJSON());
}

describe("import is atomic", () => {
  test("an op that fails while it is applied rolls back history and state", () => {
    const { source, update } = base();
    const target = doc(2);
    target.import(update);
    target.getMovableList("list").push("d");
    target.commit();
    const before = observe(target);
    const events: LoroEventBatch[] = [];
    target.subscribe((event) => events.push(event));

    expect(() => target.importJsonUpdates(forgedMove(source, 9))).toThrow(
      /out of range/u,
    );
    expect(observe(target)).toEqual(before);
    expect(events).toEqual([]);
    assertUsable(target);
  });

  test("importBatch applies no blob when one fails", () => {
    const { source, update } = base();
    const target = doc(2);
    target.import(update);
    const before = observe(target);
    const valid = doc(3);
    valid.import(update);
    valid.getMovableList("list").push("valid");
    valid.commit();
    const validUpdate = valid.export({ mode: "update", from: target.oplogVersion() });
    // A detached import only records history, which lets it carry the forged op.
    const forged = new LoroDoc();
    forged.import(update);
    forged.detach();
    forged.importJsonUpdates(forgedMove(source, 9));
    const forgedUpdate = forged.export({
      mode: "updates-in-range",
      spans: [{ id: { peer: "1", counter: 3 }, len: 1 }],
    });

    expect(() => target.importBatch([validUpdate, forgedUpdate])).toThrow(
      /out of range/u,
    );
    expect(observe(target)).toEqual(before);
    expect(() => target.importBatch([validUpdate, new Uint8Array([1, 2, 3])])).toThrow(
      /too short/u,
    );
    expect(observe(target)).toEqual(before);
    assertUsable(target);
  });

  test("a failed import keeps the pending changes it would have unlocked", () => {
    const { source, update } = base();
    const target = doc(2);
    target.import(update);
    const version = source.oplogVersion();
    // The forged move (counter 3) and a valid change after it (counter 4).
    const forged = forgedMove(source, 9);
    source.getMovableList("list").push("later");
    source.commit();
    const later = source.export({
      mode: "updates-in-range",
      spans: [{ id: { peer: "1", counter: 4 }, len: 1 }],
    });
    const pendingStatus = target.import(later);
    expect(pendingStatus.pending).not.toBeNull();
    const before = observe(target);

    expect(() => target.importJsonUpdates(forged)).toThrow(/out of range/u);
    expect(observe(target)).toEqual(before);
    // The pending change is still pending and unlocks once its dependency is valid.
    const honest = doc(1);
    honest.import(update);
    honest.getMovableList("list").move(0, 2);
    honest.commit();
    target.import(
      honest.export({ mode: "update", from: new VersionVector(version.toJSON()) }),
    );
    expect(target.getMovableList("list").toJSON()).toEqual(["b", "c", "a", "later"]);
  });

  test("a checkout that fails keeps the document where it was", () => {
    const { source, update } = base();
    const target = doc(2);
    target.import(update);
    target.detach();
    const before = observe(target);
    // A detached import only touches history; the reattaching checkout applies
    // it and fails. Rust panics here (context/movable-list-op-validation.md).
    target.importJsonUpdates(forgedMove(source, 9));
    expect(() => target.attach()).toThrow(/out of range/u);
    expect(target.isDetached()).toBe(true);
    expect(target.toJSON()).toEqual(before.value);
    expect(target.frontiers()).toEqual(before.frontiers);
  });

  test("a failed snapshot import leaves an empty document empty", () => {
    const { source } = base();
    const snapshot = source.export({ mode: "snapshot" });
    const corrupted = snapshot.slice(0, snapshot.length - 8);
    const target = new LoroDoc();
    const list = target.getMovableList("list");
    expect(() => target.import(corrupted)).toThrow(/checksum/u);
    expect(target.toJSON()).toEqual({ list: [] });
    expect(target.changeCount()).toBe(0);
    expect(list.length).toBe(0);
    target.import(snapshot);
    expect(target.toJSON()).toEqual(source.toJSON());
  });

  test("a failed import into a snapshot-loaded document rolls back its completion", () => {
    const { source, update } = base();
    source.getMovableList("list").push("d");
    source.commit();
    const hydrated = LoroDoc.fromSnapshot(source.export({ mode: "snapshot" }));
    const before = observe(hydrated);
    // Peer 2 has not seen "d", so its move is concurrent with the snapshot and
    // importing it first rebuilds the list from its history
    // (#prepareSnapshotImport).
    const concurrent = doc(2);
    concurrent.import(update);
    concurrent.getMovableList("list").move(2, 0);
    concurrent.commit();
    const concurrentUpdate = concurrent.export({ mode: "update" });
    const forged = new LoroDoc();
    forged.import(source.export({ mode: "update" }));
    forged.detach();
    forged.importJsonUpdates(forgedMove(source, 9));
    const forgedUpdate = forged.export({
      mode: "updates-in-range",
      spans: [{ id: { peer: "1", counter: 4 }, len: 1 }],
    });

    expect(() => hydrated.importBatch([concurrentUpdate, forgedUpdate])).toThrow(
      /out of range/u,
    );
    expect(observe(hydrated)).toEqual(before);
    hydrated.import(concurrentUpdate);
    const expected = doc(3);
    expected.import(update);
    expected.import(hydrated.export({ mode: "update" }));
    expect(hydrated.toJSON()).toEqual(expected.toJSON());
    expect(hydrated.toJSON()).toEqual({ list: ["c", "a", "b", "d"] });
    assertUsable(hydrated);
  });

  test("undo is unaffected by a failed import", () => {
    const { source, update } = base();
    const target = doc(2);
    target.import(update);
    const undo = new UndoManager(target, { mergeInterval: 0 });
    target.getMovableList("list").push("mine");
    target.commit();
    expect(() => target.importJsonUpdates(forgedMove(source, 9))).toThrow(
      /out of range/u,
    );
    expect(undo.undo()).toBe(true);
    expect(target.getMovableList("list").toJSON()).toEqual(["a", "b", "c"]);
  });

  test("an import costs the same however many texts a snapshot hydrated", () => {
    /** Best of three runs of 200 one-character imports into a document with n hydrated texts. */
    function importsMs(size: number): number {
      const source = doc(1);
      const items = source.getList("items");
      for (let index = 0; index < size; index += 1) {
        const item = items.insertContainer(index, new LoroMap());
        item.setContainer("title", new LoroText()).insert(0, "t");
      }
      source.commit();
      const target = new LoroDoc();
      // A subscriber hydrates every text, and each becomes a snapshot sequence.
      target.subscribe(() => {});
      target.import(source.export({ mode: "snapshot" }));
      const title = (items.get(0) as LoroMap).get("title") as LoroText;
      const updates: Uint8Array[] = [];
      for (let step = 0; step < 600; step += 1) {
        const version = source.oplogVersion();
        title.insert(0, "x");
        source.commit();
        updates.push(source.export({ mode: "update", from: version }));
      }
      let best = Infinity;
      for (let run = 0; run < 3; run += 1) {
        const started = performance.now();
        for (const update of updates.slice(run * 200, run * 200 + 200)) {
          target.import(update);
        }
        best = Math.min(best, performance.now() - started);
      }
      expect(target.toJSON()).toEqual(source.toJSON());
      return best;
    }
    // Each import copied the table of snapshot sequences to roll it back
    // (16x the texts took about 10x the time).
    const ratio = importsMs(8_000) / importsMs(500);
    expect(ratio).toBeLessThan(4);
  }, 60_000);
});
