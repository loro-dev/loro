import { describe, expect, test } from "vitest";

import { LoroDoc, LoroText, UndoManager } from "../src/index";

/**
 * Mergeable children in diff/applyDiff, aligned with Rust main
 * (loro-dev/loro#1134). Expected values were produced by the Rust WASM build.
 */
type Kind = "Text" | "Counter" | "List" | "Map";
const KINDS: readonly Kind[] = ["Text", "Counter", "List", "Map"];

const plain = (value: unknown): unknown => JSON.parse(JSON.stringify(value));

function ensure(doc: LoroDoc, kind: Kind) {
  const map = doc.getMap("m");
  if (kind === "Text") return map.ensureMergeableText("s");
  if (kind === "Counter") return map.ensureMergeableCounter("s");
  if (kind === "List") return map.ensureMergeableList("s");
  return map.ensureMergeableMap("s");
}

/** Writes the kind's value, commits, deletes the key, and returns the first version. */
function deletedChild(kind: Kind) {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const child = ensure(doc, kind);
  if (child instanceof LoroText) child.insert(0, "hello");
  else if ("increment" in child) child.increment(7);
  else if ("push" in child) child.push("keep");
  else child.set("k", 1);
  doc.commit();
  const alive = doc.frontiers();
  doc.getMap("m").delete("s");
  doc.commit();
  return { doc, alive };
}

/** A receiver whose mergeable child holds different state. */
function receiver(kind: Kind): LoroDoc {
  const doc = new LoroDoc();
  doc.setPeerId(2);
  const child = ensure(doc, kind);
  if (child instanceof LoroText) child.insert(0, "help me");
  else if ("increment" in child) child.increment(3);
  else if ("push" in child) {
    child.push("drop");
    child.push("keep");
  } else {
    child.set("k", 2);
    child.set("z", 0);
  }
  doc.commit();
  return doc;
}

const EXPECTED: Record<
  Kind,
  { diff: unknown; incremental: unknown; fullState: unknown; alive: unknown }
> = {
  Text: {
    diff: { type: "text", diff: [{ insert: "hello" }] },
    incremental: "hellohelp me",
    fullState: "hello",
    alive: "hello",
  },
  Counter: {
    diff: { type: "counter", increment: 7 },
    incremental: 10,
    fullState: 7,
    alive: 7,
  },
  List: {
    diff: { type: "list", diff: [{ insert: ["keep"] }] },
    incremental: ["keep", "drop", "keep"],
    fullState: ["keep"],
    alive: ["keep"],
  },
  Map: {
    diff: { type: "map", updated: { k: 1 } },
    incremental: { k: 1, z: 0 },
    fullState: { k: 1 },
    alive: { k: 1 },
  },
};

describe("mergeable children in diff and applyDiff", () => {
  for (const kind of KINDS) {
    const cid = `cid:root-🤝:$m>s:${kind}`;

    test(`diff reports a re-activated ${kind} with its full state`, () => {
      const { doc, alive } = deletedChild(kind);
      const diff = doc.diff(doc.frontiers(), alive);
      expect(diff.map(([id]) => id)).toEqual(["cid:root-m:Map", cid]);
      expect(plain(diff[1]![1])).toEqual(EXPECTED[kind].diff);
    });

    test(`applyDiff of a ${kind} is incremental unless fullState is set`, () => {
      const { doc, alive } = deletedChild(kind);
      const diff = doc.diff(doc.frontiers(), alive);
      for (const fullState of [false, true]) {
        const target = receiver(kind);
        target.applyDiff(diff, fullState ? { fullState } : undefined);
        target.commit();
        expect(target.toJSON()).toEqual({
          m: { s: EXPECTED[kind][fullState ? "fullState" : "incremental"] },
        });
      }
    });

    test(`revertTo re-activates a ${kind} without duplicating its state`, () => {
      const { doc, alive } = deletedChild(kind);
      doc.revertTo(alive);
      doc.commit();
      expect(doc.toJSON()).toEqual({ m: { s: EXPECTED[kind].alive } });
    });
  }

  test("diff has no entry for a hidden mergeable child", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const start = doc.frontiers();
    doc.getMap("m").ensureMergeableText("s").insert(0, "hello");
    doc.commit();
    doc.getMap("m").delete("s");
    doc.commit();
    // Rust reports the parent key as removed (`undefined`) and no text entry.
    expect(plain(doc.diff(start, doc.frontiers()))).toEqual([
      ["cid:root-m:Map", { type: "map", updated: {} }],
    ]);
  });

  test("a local re-ensure emits only the parent marker; checkout emits full state", () => {
    const { doc } = deletedChild("Text");
    const deleted = doc.frontiers();
    const batches: unknown[] = [];
    doc.subscribe((batch) =>
      batches.push([batch.by, batch.events.map((event) => [event.target, event.diff])]),
    );
    doc.getMap("m").ensureMergeableText("s");
    doc.commit();
    doc.checkout(deleted);
    batches.length = 1;
    doc.checkoutToLatest();
    expect(plain(batches)).toEqual([
      ["local", [["cid:root-m:Map", { type: "map", updated: { s: "hello" } }]]],
      [
        "checkout",
        [
          ["cid:root-m:Map", { type: "map", updated: { s: "hello" } }],
          ["cid:root-🤝:$m>s:Text", { type: "text", diff: [{ insert: "hello" }] }],
        ],
      ],
    ]);
  });

  test("undo of a delete only restores visibility", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const undo = new UndoManager(doc, { mergeInterval: 0 });
    doc.getMap("m").ensureMergeableText("s").insert(0, "hello");
    doc.commit();
    doc.getMap("m").delete("s");
    doc.commit();
    const batches: unknown[] = [];
    doc.subscribe((batch) =>
      batches.push([batch.by, batch.events.map((event) => [event.target, event.diff])]),
    );
    undo.undo();
    expect(doc.toJSON()).toEqual({ m: { s: "hello" } });
    expect(plain(batches)).toEqual([
      ["local", [["cid:root-m:Map", { type: "map", updated: { s: "hello" } }]]],
    ]);
  });
});

describe("batch validation of a hidden mergeable list (Rust main results)", () => {
  for (const kind of ["List", "MovableList"] as const) {
    const ensureList = (doc: LoroDoc) =>
      kind === "List"
        ? doc.getMap("m").ensureMergeableList("s")
        : doc.getMap("m").ensureMergeableMovableList("s");

    test(`revertTo re-activates a ${kind} edited remotely while hidden`, () => {
      const doc = new LoroDoc();
      doc.setPeerId(1);
      const list = ensureList(doc);
      list.insert(0, 1);
      list.insert(1, 2);
      doc.commit();
      const alive = doc.frontiers();
      const remote = new LoroDoc();
      remote.setPeerId(2);
      remote.import(doc.export({ mode: "snapshot" }));
      doc.getMap("m").delete("s");
      doc.commit();
      ensureList(remote).insert(2, 9);
      remote.commit();
      doc.import(remote.export({ mode: "update" }));
      doc.revertTo(alive);
      doc.commit();
      expect(doc.toJSON()).toEqual({ m: { s: [1, 2] } });
    });

    test(`an event mirror accepts a re-ensured ${kind} edited in the same commit`, () => {
      const doc = new LoroDoc();
      doc.setPeerId(1);
      const mirror = new LoroDoc();
      mirror.setPeerId(9);
      doc.subscribe((batch) => {
        mirror.applyDiff(batch.events.map((event) => [event.target, event.diff]));
        mirror.commit();
      });
      const list = ensureList(doc);
      list.insert(0, 1);
      list.insert(1, 2);
      doc.commit();
      doc.getMap("m").delete("s");
      doc.commit();
      ensureList(doc).insert(1, "new");
      doc.commit();
      expect(mirror.toJSON()).toEqual({ m: { s: [1, "new", 2] } });
    });

    test(`a delta beyond a hidden ${kind}'s length is still rejected`, () => {
      const doc = new LoroDoc();
      doc.setPeerId(1);
      const list = ensureList(doc);
      list.insert(0, 1);
      list.insert(1, 2);
      doc.commit();
      doc.getMap("m").delete("s");
      doc.commit();
      const id = `cid:root-🤝:$m>s:${kind}` as const;
      expect(() =>
        doc.applyDiff([
          ["cid:root-m:Map", { type: "map", updated: { s: `🦜:${id}` } }],
          [id, { type: "list", diff: [{ retain: 3 }, { insert: ["x"] }] }],
        ]),
      ).toThrow(/consumes 3 items but the list has 2/);
      expect(doc.toJSON()).toEqual({ m: {} });
    });
  }
});
