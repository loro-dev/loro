import { describe, expect, it } from "vitest";
import { LoroDoc, LoroMap, LoroText, TreeID } from "../bundler/index";

// A failed import must not leave state behind for containers created later: a new root
// map used to show the text of a tree node's metadata that the failed import had loaded
// while it validated. See loro-dev/loro#1164 and context/failed-import-arena-indices.md.

const NODES = 40;

function snapshotAndBadUpdate(): { snap: Uint8Array; bad: Uint8Array } {
  // Peer 2 creates nodes under `p` and puts a text in their metas; peer 1 deletes `p`
  // concurrently.
  const a = new LoroDoc();
  a.setPeerId(1);
  const p = a.getTree("tree").createNode();
  a.commit();
  const b = new LoroDoc();
  b.setPeerId(2);
  b.import(a.export({ mode: "update" }));
  const nodes: TreeID[] = [];
  for (let i = 0; i < NODES; i++) {
    const c = b.getTree("tree").getNodeByID(p.id)!.createNode();
    c.data.setContainer("t", new LoroText()).insert(0, `z${i}`);
    nodes.push(c.id);
  }
  b.commit();
  a.getTree("tree").delete(p.id);
  a.commit();
  a.import(b.export({ mode: "update" }));
  a.getMap("m").set("k", 1);
  a.commit();
  const snap = a.export({ mode: "snapshot" });
  const vv = a.oplogVersion();

  // Peer 4 revives the nodes and edits their metas, then a text insert forged out of
  // bounds, which the state rejects. Which diffs the state validates before it rejects
  // the text depends on hash order (it differs between wasm32 and native), so there are
  // many metas.
  const e = new LoroDoc();
  e.setPeerId(4);
  e.import(snap);
  for (const id of nodes) {
    e.getTree("tree").move(id, undefined);
    e.getTree("tree").getNodeByID(id)!.data.set("r", 1);
  }
  e.getText("text").insert(0, "a");
  e.commit();
  const json = e.exportJsonUpdates(vv, e.oplogVersion());
  const ops = json.changes[json.changes.length - 1].ops;
  (ops[ops.length - 1].content as { pos: number }).pos = 1000;
  const carrier = new LoroDoc();
  carrier.import(snap);
  carrier.detach();
  carrier.importJsonUpdates(json);
  const bad = carrier.export({ mode: "update", from: vv });
  return { snap, bad };
}

function load(bytes: Uint8Array): LoroDoc {
  const doc = new LoroDoc();
  doc.import(bytes);
  doc.setPeerId(99);
  return doc;
}

describe("failed import state rollback", () => {
  it("containers created after a failed import get their own state", () => {
    const { snap, bad } = snapshotAndBadUpdate();
    const doc = load(snap);
    const reference = load(snap);
    expect(() => doc.import(bad)).toThrow();
    expect(() => doc.import(bad)).toThrow();
    for (const d of [doc, reference]) {
      for (let i = 0; i < 2 * NODES; i++) {
        const child = d.getMap(`fresh${i}`).setContainer("c", new LoroMap());
        child.set("v", i);
        d.commit();
        expect(child.isDeleted()).toBe(false);
      }
    }
    expect(doc.toJSON()).toEqual(reference.toJSON());
    expect(load(doc.export({ mode: "snapshot" })).toJSON()).toEqual(
      reference.toJSON(),
    );
  });
});
