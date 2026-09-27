import { describe, expect, it } from "vitest";
import { ContainerID, LoroCounter, LoroDoc, LoroText } from "../bundler/index";

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
});
