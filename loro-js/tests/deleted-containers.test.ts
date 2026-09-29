import { describe, expect, test } from "vitest";

import { LoroDoc, LoroMap, LoroText } from "../src/index";

// A container is deleted when any ancestor was removed from its parent, and a
// tree node is deleted when any ancestor node is. Expected values are what
// loro-crdt returns for the same calls. (loro-crdt also rejects local edits to
// a deleted container; loro.js still accepts them.)

describe("deleted containers and tree nodes", () => {
  test("treat the subtree of a deleted tree node as deleted", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const tree = doc.getTree("tree");
    const parent = tree.createNode();
    const child = tree.createNode(parent.id);
    const grandchild = tree.createNode(child.id);
    const other = tree.createNode();
    doc.commit();
    tree.delete(parent.id);
    doc.commit();

    expect(tree.getNodes().map((node) => node.id)).toEqual([other.id]);
    expect(
      tree
        .getNodes({ withDeleted: true })
        .map((node) => node.id)
        .sort(),
    ).toEqual([parent.id, child.id, grandchild.id, other.id].sort());
    expect(
      tree
        .nodes()
        .map((node) => node.id)
        .sort(),
    ).toEqual([parent.id, child.id, grandchild.id, other.id].sort());
    expect(tree.isNodeDeleted(parent.id)).toBe(true);
    expect(tree.isNodeDeleted(child.id)).toBe(true);
    expect(tree.getNodeByID(grandchild.id)!.isDeleted()).toBe(true);
    expect(tree.getNodeByID(other.id)!.isDeleted()).toBe(false);
    expect(grandchild.data.isDeleted()).toBe(true);
    expect(doc.getPathToContainer(grandchild.data.id)).toBeUndefined();

    // Moving a node out of the deleted subtree revives it and its children.
    // (loro-crdt 1.16 keeps the metadata of a node it already reported as
    // deleted marked deleted after such a local move; a reload clears it.)
    tree.move(child.id, undefined, 0);
    doc.commit();
    expect(tree.isNodeDeleted(grandchild.id)).toBe(false);
    grandchild.data.set("key", 1);
    doc.commit();
    expect(tree.getNodes().map((node) => node.id)).toEqual([
      child.id,
      other.id,
      grandchild.id,
    ]);
  });

  test("treat children of a removed map value as deleted", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const root = doc.getMap("root");
    const map = root.setContainer("map", new LoroMap());
    const empty = map.setContainer("empty", new LoroText());
    const text = map.setContainer("text", new LoroText());
    text.insert(0, "x");
    doc.commit();
    root.delete("map");
    doc.commit();

    for (const container of [map, empty, text]) {
      expect(container.isDeleted()).toBe(true);
      expect(doc.getPathToContainer(container.id)).toBeUndefined();
      expect(doc.getContainerById(container.id)).toBeDefined();
    }
  });

  test("treats a list child removed from its list as deleted", () => {
    const doc = new LoroDoc();
    const list = doc.getList("list");
    const child = list.insertContainer(0, new LoroMap());
    const grandchild = child.setContainer("text", new LoroText());
    doc.commit();
    list.delete(0, 1);
    doc.commit();
    expect(child.isDeleted()).toBe(true);
    expect(grandchild.isDeleted()).toBe(true);
    expect(doc.getPathToContainer(grandchild.id)).toBeUndefined();
  });
});
