import { describe, expect, it } from "vitest";
import { LoroDoc, LoroMap } from "../bundler/index";

// A container that was reported deleted must count as alive again once it is
// revived. See context/dead-container-cache.md.

function deletedSubtree() {
  const doc = new LoroDoc();
  doc.setPeerId(1n);
  const tree = doc.getTree("tree");
  const parent = tree.createNode();
  const child = tree.createNode(parent.id);
  const grandchild = tree.createNode(child.id);
  doc.commit();
  tree.delete(parent.id);
  doc.commit();

  const meta = tree.getNodeByID(grandchild.id)!.data;
  expect(meta.isDeleted()).toBe(true);
  expect(() => {
    meta.set("k", 1);
    doc.commit();
  }).toThrow();
  return { doc, tree, child, grandchild, meta };
}

describe("dead container cache", () => {
  it("revives tree metadata after a local move", () => {
    const { doc, tree, child, grandchild, meta } = deletedSubtree();
    tree.move(child.id, undefined, 0);
    doc.commit();

    expect(tree.isNodeDeleted(grandchild.id)).toBe(false);
    expect(meta.isDeleted()).toBe(false);
    meta.set("key", 1);
    doc.commit();
    expect(tree.getNodeByID(grandchild.id)!.data.get("key")).toBe(1);
  });

  it("revives tree metadata after an imported move", () => {
    const { doc, child, grandchild, meta } = deletedSubtree();
    const other = new LoroDoc();
    other.setPeerId(2n);
    other.import(doc.export({ mode: "update" }));
    other.getTree("tree").move(child.id, undefined, 0);
    other.commit();

    doc.import(other.export({ mode: "update", from: doc.oplogVersion() }));
    expect(doc.getTree("tree").isNodeDeleted(grandchild.id)).toBe(false);
    expect(meta.isDeleted()).toBe(false);
    meta.set("key", 1);
    doc.commit();
  });

  it("revives a movable list child after importing a concurrent move", () => {
    const a = new LoroDoc();
    a.setPeerId(1n);
    const list = a.getMovableList("list");
    list.insert(0, 0);
    const child = list.insertContainer(1, new LoroMap());
    child.set("x", 1);
    a.commit();

    const b = new LoroDoc();
    b.setPeerId(2n);
    b.import(a.export({ mode: "update" }));
    b.getMap("pad").set("k", 1);
    b.commit();
    b.getMovableList("list").move(1, 0);
    b.commit();

    list.delete(1, 1);
    a.commit();
    expect(child.isDeleted()).toBe(true);

    a.import(b.export({ mode: "update" }));
    expect(list.length).toBe(2);
    expect(child.isDeleted()).toBe(false);
    child.set("y", 2);
    a.commit();
  });
});
