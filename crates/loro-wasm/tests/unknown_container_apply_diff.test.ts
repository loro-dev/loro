import { describe, expect, it } from "vitest";
import {
  ContainerID,
  LoroCounter,
  LoroDoc,
  LoroEventBatch,
  LoroList,
  LoroMap,
  LoroText,
  UndoManager,
} from "../bundler/index";
import {
  UNKNOWN_MERGEABLE_HOLDERS,
  UNKNOWN_REVIEW_DOCS,
} from "./fixtures/unknown_mergeable_holders";

// `applyDiff` used to trap the wasm instance (`unreachable!()` in
// `Handler::new_unattached`) when a diff asked it to create a container whose
// type is unknown to this version. It must throw a readable error and leave the
// doc untouched, while docs that already hold unknown containers keep working.
const UNKNOWN_CIDS = [
  "🦜:cid:0@1:Unknown(9)",
  "🦜:cid:root-future:Unknown(9)",
] as const;

function expectRejected(doc: LoroDoc, diff: [ContainerID, unknown][]) {
  doc.commit();
  const before = doc.toJSON();
  const version = doc.version().toJSON();
  // Applied first, so partial application would be visible
  const witness: [ContainerID, unknown] = [
    "cid:root-witness:Text",
    { type: "text", diff: [{ insert: "partial" }] },
  ];
  expect(() => doc.applyDiff([witness, ...diff] as never)).toThrowError(
    /Unknown\(9\)/,
  );
  doc.commit();
  expect(doc.toJSON()).toStrictEqual(before);
  expect(doc.version().toJSON()).toStrictEqual(version);
  // The instance is still usable
  doc.getText("after").insert(0, "ok");
  doc.commit();
  expect(doc.getText("after").toString()).toBe("ok");
}

/**
 * Replays `build`'s history into a new doc with every Counter turned into an
 * `Unknown(9)` container, as if a newer loro-crdt had written it.
 */
function forge(build: (doc: LoroDoc) => void): LoroDoc {
  const src = new LoroDoc();
  src.setPeerId(1);
  build(src);
  src.commit();
  const json = JSON.stringify(src.exportJsonUpdates())
    .replace(/(cid:\d+@\d+):Counter/g, "$1:Unknown(9)")
    .split('"type":"counter"')
    .join('"type":"unknown"');
  const doc = new LoroDoc();
  doc.setPeerId(2);
  doc.importJsonUpdates(JSON.parse(json));
  return doc;
}

/** `m.k = List[U, 1]` with an unknown container U. */
function listHoldingUnknown(): LoroDoc {
  const doc = forge((src) => {
    const list = src.getMap("m").setContainer("k", new LoroList());
    list.insertContainer(0, new LoroCounter()).increment(1);
    list.push(1);
  });
  expect(doc.toJSON()).toStrictEqual({ m: { k: [null, 1] } });
  return doc;
}

function state(doc: LoroDoc) {
  doc.commit();
  return [doc.toJSON(), doc.version().toJSON()];
}

describe("applyDiff with unknown container types", () => {
  for (const cid of UNKNOWN_CIDS) {
    it(`rejects inserting ${cid} into a List`, () => {
      const doc = new LoroDoc();
      doc.getList("list").push(1);
      expectRejected(doc, [
        ["cid:root-list:List", { type: "list", diff: [{ insert: [cid] }] }],
      ]);
    });

    it(`rejects inserting ${cid} into a MovableList`, () => {
      const doc = new LoroDoc();
      doc.getMovableList("list").push(1);
      expectRejected(doc, [
        [
          "cid:root-list:MovableList",
          { type: "list", diff: [{ retain: 1 }, { insert: [cid] }] },
        ],
      ]);
    });

    it(`rejects setting ${cid} in a Map`, () => {
      const doc = new LoroDoc();
      doc.getMap("map").set("a", 1);
      expectRejected(doc, [
        ["cid:root-map:Map", { type: "map", updated: { k: cid } }],
      ]);
    });

    it(`rejects setting ${cid} in a tree node's meta`, () => {
      const doc = new LoroDoc();
      doc.setPeerId(1);
      const node = doc.getTree("tree").createNode();
      node.data.set("a", 1);
      expectRejected(doc, [
        [`cid:${node.id}:Map`, { type: "map", updated: { k: cid } }],
      ]);
    });

    it(`rejects ${cid} nested in containers created by the same diff`, () => {
      const doc = new LoroDoc();
      doc.getMap("map").set("a", 1);
      expectRejected(doc, [
        [
          "cid:root-map:Map",
          {
            type: "map",
            updated: { k: "🦜:cid:100@5:List", m: "🦜:cid:101@5:Map" },
          },
        ],
        ["cid:100@5:List", { type: "list", diff: [{ insert: [1] }] }],
        ["cid:101@5:Map", { type: "map", updated: { u: cid } }],
      ]);
    });
  }

  it("keeps docs that already hold unknown containers working", () => {
    // Forge a container written by a newer Loro by rewriting JSON updates
    const src = new LoroDoc();
    src.setPeerId(1);
    const list = src.getMovableList("mlist");
    list.push("a");
    list.insertContainer(0, new LoroText());
    list.push("b");
    src.commit();
    const patched = JSON.parse(
      JSON.stringify(src.exportJsonUpdates()).split("cid:1@0:Text").join(
        "cid:1@0:Unknown(9)",
      ),
    );
    expect(JSON.stringify(patched)).toContain("Unknown(9)");
    const doc = new LoroDoc();
    doc.setPeerId(2);
    doc.importJsonUpdates(patched);
    const mlist = doc.getMovableList("mlist");
    expect(doc.toJSON()).toStrictEqual({ mlist: [null, "a", "b"] });

    const v0 = doc.frontiers();
    mlist.move(0, 2);
    doc.commit();
    const v1 = doc.frontiers();
    expect(doc.toJSON()).toStrictEqual({ mlist: ["a", "b", null] });

    // Moves of an existing unknown element don't create containers
    doc.revertTo(v0);
    doc.commit();
    expect(doc.toJSON()).toStrictEqual({ mlist: [null, "a", "b"] });
    doc.applyDiff(doc.diff(v0, v1));
    doc.commit();
    expect(doc.toJSON()).toStrictEqual({ mlist: ["a", "b", null] });

    const copy = new LoroDoc();
    copy.import(doc.export({ mode: "snapshot" }));
    expect(copy.toJSON()).toStrictEqual(doc.toJSON());

    // Recreating a deleted unknown element is rejected without changes
    const v2 = doc.frontiers();
    mlist.delete(2, 1);
    doc.commit();
    const before = doc.toJSON();
    expect(() => doc.revertTo(v2)).toThrowError(/Unknown\(9\)/);
    doc.commit();
    expect(doc.toJSON()).toStrictEqual(before);
  });

  it("imports and checks out an unknown container created and edited together", () => {
    // A newer Loro creates `map.k` with an unknown type and edits it in the
    // same change; forged from a counter's JSON updates.
    const src = new LoroDoc();
    src.setPeerId(1);
    src.getMap("map").setContainer("k", new LoroCounter()).increment(2);
    src.commit();
    const json = JSON.stringify(src.exportJsonUpdates())
      .split("cid:0@0:Counter")
      .join("cid:0@0:Unknown(9)")
      .split('"type":"counter"')
      .join('"type":"unknown"');
    expect(json).toContain("Unknown(9)");
    expect(json).not.toContain('"type":"counter"');

    const forged = new LoroDoc();
    forged.subscribe(() => {});
    forged.importJsonUpdates(JSON.parse(json));
    for (const mode of ["update", "snapshot"] as const) {
      const doc = new LoroDoc();
      doc.subscribe(() => {});
      doc.import(forged.export({ mode }));
      expect(doc.toJSON()).toStrictEqual({ map: { k: null } });
      doc.checkout([]);
      expect(doc.getMap("map").get("k")).toBeUndefined();
      doc.checkoutToLatest();
      expect(doc.toJSON()).toStrictEqual({ map: { k: null } });
    }
  });

  it("rejects recreating a deleted list holding an unknown container atomically", () => {
    const doc = listHoldingUnknown();
    const v0 = doc.frontiers();
    doc.getText("t").insert(0, "x");
    doc.getMap("m").delete("k");
    doc.commit();
    const v1 = doc.frontiers();
    const before = state(doc);
    // The recreated list `m.k` would get U: used to apply part of the diff
    // (`m.k` became `[]`) before failing
    expect(() => doc.applyDiff(doc.diff(v1, v0))).toThrowError(
      /Unknown\(9\)/,
    );
    expect(state(doc)).toStrictEqual(before);
    expect(() => doc.revertTo(v0)).toThrowError(/Unknown\(9\)/);
    expect(state(doc)).toStrictEqual(before);
  });

  it("fails an undo step that would recreate an unknown container without changes", () => {
    const doc = listHoldingUnknown();
    const undo = new UndoManager(doc, { mergeInterval: 0 });
    doc.getText("t").insert(0, "a");
    doc.commit();
    // One step mixing a text edit with deleting the list holding U
    doc.getText("t").insert(1, "b");
    doc.getMap("m").delete("k");
    doc.commit();

    // Used to return true after dropping both U and `1`
    const before = state(doc);
    expect(() => undo.undo()).toThrowError(/Unknown\(9\)/);
    expect(state(doc)).toStrictEqual(before);
    // The step is dropped; the next undo undoes only the step before it
    expect(undo.canUndo()).toBe(true);
    expect(undo.undo()).toBe(true);
    expect(doc.toJSON()).toStrictEqual({ m: {}, t: "b" });
    expect(undo.canUndo()).toBe(false);
  });

  it("still emits the events of known containers next to unknown ones", async () => {
    // A newer loro-crdt creates `m.u` with an unknown type, edits it, and
    // edits a text in the same change
    const src = new LoroDoc();
    src.setPeerId(1);
    src.getMap("m").setContainer("u", new LoroCounter()).increment(1);
    src.getText("t").insert(0, "hi");
    src.commit();
    const json = JSON.stringify(src.exportJsonUpdates())
      .replace(/(cid:\d+@\d+):Counter/g, "$1:Unknown(9)")
      .split('"type":"counter"')
      .join('"type":"unknown"');

    const doc = new LoroDoc();
    const batches: LoroEventBatch[] = [];
    doc.subscribe((e) => batches.push(e));
    doc.importJsonUpdates(JSON.parse(json));
    await Promise.resolve();
    // The whole batch used to be dropped
    expect(batches.length).toBe(1);
    const targets = batches[0].events.map((e) => e.target);
    expect(targets).toContain("cid:root-t:Text");
    expect(targets.some((t) => t.includes("Unknown"))).toBe(false);
    expect(doc.toJSON()).toStrictEqual({ m: { u: null }, t: "hi" });
  });

  // Full-state batches from `doc.diff()` (`{ fullState: true }`) align a
  // re-activated mergeable child with the state this doc kept for it.
  // Keeping an unknown container there creates nothing.
  const HOLDERS = ["map", "list", "movableList", "treeMeta"] as const;
  type Holder = (typeof HOLDERS)[number];

  /** A doc whose mergeable `m.s` holds an unknown container. JSON updates
   * can't carry the binary mergeable markers, so it is forged in Rust. */
  function forgeHolder(holder: Holder): LoroDoc {
    const bytes = Uint8Array.from(atob(UNKNOWN_MERGEABLE_HOLDERS[holder]), (c) =>
      c.charCodeAt(0),
    );
    const doc = new LoroDoc();
    doc.setPeerId(2);
    doc.import(bytes);
    expect(Object.keys(doc.toJSON().m)).toStrictEqual(["s"]);
    return doc;
  }

  for (const holder of HOLDERS) {
    it(`fullState applyDiff keeps an existing unknown container in a mergeable ${holder}`, () => {
      const doc = forgeHolder(holder);
      const target = doc.frontiers();
      const expected = doc.toJSON().m;
      doc.getText("t").insert(0, "x");
      doc.getMap("m").delete("s");
      doc.commit();
      // Used to be rejected as creating the unknown container
      doc.applyDiff(doc.diff(doc.frontiers(), target), { fullState: true });
      expect(doc.toJSON().m).toStrictEqual(expected);
    });

    it(`fullState applyDiff that has to create an unknown container in a mergeable ${holder} is rejected`, () => {
      const src = forgeHolder(holder);
      const target = src.frontiers();
      src.getMap("m").delete("s");
      src.commit();
      const diff = src.diff(src.frontiers(), target);

      // `m.s` has no hidden state here
      const doc = new LoroDoc();
      doc.getText("t").insert(0, "x");
      doc.commit();
      const before = state(doc);
      const witness: [ContainerID, unknown] = [
        "cid:root-t:Text",
        { type: "text", diff: [{ insert: "partial" }] },
      ];
      expect(() =>
        doc.applyDiff([witness, ...diff] as never, { fullState: true }),
      ).toThrowError(/Unknown\(9\)/);
      expect(state(doc)).toStrictEqual(before);
    });
  }

  // Reproductions from the second review of #1142
  function loadReviewDoc(name: keyof typeof UNKNOWN_REVIEW_DOCS): LoroDoc {
    const doc = new LoroDoc();
    doc.setPeerId(2);
    doc.import(
      Uint8Array.from(atob(UNKNOWN_REVIEW_DOCS[name]), (c) => c.charCodeAt(0)),
    );
    return doc;
  }

  it("rejects reviving a mergeable child of a recreated map atomically", () => {
    const doc = loadReviewDoc("recreatedParent");
    const v0 = doc.frontiers();
    doc.getMap("r").delete("m");
    doc.commit();
    const diff = doc.diff(doc.frontiers(), v0);
    for (const fullState of [true, false]) {
      const before = state(doc);
      // Used to write `r.m = { k: [] }` before failing
      expect(() => doc.applyDiff(diff, { fullState })).toThrowError(
        /Unknown\(9\)/,
      );
      expect(state(doc)).toStrictEqual(before);
    }
  });

  it("rejects an undo that revives a mergeable parent of a deleted unknown container", () => {
    const doc = loadReviewDoc("hiddenMergeableChild");
    const undo = new UndoManager(doc, { mergeInterval: 0 });
    const s = doc.getMap("m").get("s") as LoroMap;
    (s.get("l") as LoroList).delete(0, 1);
    doc.getText("t").insert(0, "x");
    doc.getMap("m").delete("s");
    doc.commit();
    const before = state(doc);
    // Used to return true with U silently gone
    expect(() => undo.undo()).toThrowError(/Unknown\(9\)/);
    expect(state(doc)).toStrictEqual(before);
  });

  it("undoes the right edits after a rejected undo step", () => {
    const doc = loadReviewDoc("unknownList");
    const undo = new UndoManager(doc, { mergeInterval: 0 });
    const t = doc.getText("t");
    t.insert(0, "a");
    doc.commit();
    t.insert(0, "b");
    doc.getList("us").delete(0, 1);
    doc.commit();
    expect(() => undo.undo()).toThrowError(/Unknown\(9\)/);
    expect(t.toString()).toBe("ba");
    // Used to delete "b" instead of "a"
    expect(undo.undo()).toBe(true);
    expect(t.toString()).toBe("b");
    expect(undo.redo()).toBe(true);
    expect(t.toString()).toBe("ba");
  });
});
