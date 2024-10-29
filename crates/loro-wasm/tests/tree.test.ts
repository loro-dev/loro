import { assert, describe, expect, it } from "vitest";
import { LoroDoc, LoroTree, LoroTreeNode, TreeDiff } from "../bundler/index";

function assertEquals(a: any, b: any) {
  expect(a).toStrictEqual(b);
}

describe("loro tree", () => {
  const loro = new LoroDoc();
  const tree = loro.getTree("root");
  tree.enableFractionalIndex(0);

  it("create", () => {
    const root = tree.createNode();
    const child = tree.createNode(root.id);
    assertEquals(child.parent()!.id, root.id);
    const child2 = tree.createNode(root.id);
    assertEquals(child.index(), 0);
    assertEquals(child2.index(), 1);
  });

  it("create with index", () => {
    const root = tree.createNode();
    const child = tree.createNode(root.id);
    assertEquals(child.parent()!.id, root.id);
    const child2 = tree.createNode(root.id, 0);
    assertEquals(child.index(), 1);
    assertEquals(child2.index(), 0);
  });

  it("move", () => {
    const root = tree.createNode();
    const child = tree.createNode(root.id);
    const child2 = tree.createNode(root.id);
    assertEquals(child2.parent()!.id, root.id);
    tree.move(child2.id, child.id);
    assertEquals(child2.parent()!.id, child.id);
    assertEquals(child.children()![0].id, child2.id);
    expect(() => tree.move(child2.id, child.id, 1)).toThrowError();
  });

  it("delete", () => {
    const root = tree.createNode();
    const child = tree.createNode(root.id);
    const child2 = tree.createNode(root.id);
    tree.delete(child.id);
    assertEquals(child2.index(), 0);
  });

  it("has", () => {
    const root = tree.createNode();
    const child = tree.createNode(root.id);
    assertEquals(tree.has(child.id), true);
    tree.delete(child.id);
    assertEquals(tree.has(child.id), true);
    assertEquals(tree.isNodeDeleted(child.id), true);
  });

  it("getNodeByID", () => {
    const root = tree.createNode();
    const child = tree.createNode(root.id);
    assertEquals(tree.getNodeByID(child.id).id, child.id);
    tree.delete(child.id);
    assertEquals(child.isDeleted(), true);
    assertEquals(tree.getNodeByID(child.id).id, child.id);
  });

  it("parent", () => {
    const root = tree.createNode();
    const child = tree.createNode(root.id);
    assertEquals(child.parent()!.id, root.id);
  });

  it("children", () => {
    const root = tree.createNode();
    const child = tree.createNode(root.id);
    const child2 = tree.createNode(root.id);
    assertEquals(root.children()!.length, 2);
    assertEquals(root.children()![0].id, child.id);
    assertEquals(root.children()![1].id, child2.id);
  });

  it("toArray", () => {
    const loro2 = new LoroDoc();
    const tree2 = loro2.getTree("root");
    const root = tree2.createNode();
    tree2.createNode(root.id);
    tree2.createNode(root.id);
    const arr = tree2.toArray();
    assertEquals(arr.length, 1);
    assertEquals(arr[0].children.length, 2)
    const keys = Object.keys(arr[0]);
    assert(keys.includes("id"));
    assert(keys.includes("parent"));
    assert(keys.includes("index"));
    assert(keys.includes("fractionalIndex"));
    assert(keys.includes("meta"));
    assert(keys.includes("children"));
  });

  it("getNodes", () => {
    const loro2 = new LoroDoc();
    const tree2 = loro2.getTree("root");
    const root = tree2.createNode();
    const child = root.createNode();
    const nodes = tree2.getNodes({ withDeleted: false });
    assertEquals(nodes.length, 2);
    assertEquals(nodes.map((n) => { return n.id }), [root.id, child.id])
    tree2.delete(child.id);
    const nodesWithDeleted = tree2.getNodes({ withDeleted: true });
    assertEquals(nodesWithDeleted.map((n) => { return n.id }), [root.id, child.id]);
    assertEquals(tree2.getNodes({ withDeleted: false }).map((n) => { return n.id }), [root.id]);
  });

  it("subscribe", async () => {
    const root = tree.createNode();
    const child: LoroTreeNode = tree.createNode(root.id);
    let count = 0;
    const sub = tree.subscribe(() => {
      count += 1;
    });
    assertEquals(count, 0);
    child.move();
    assertEquals(count, 0);
    loro.commit();
    await one_ms();
    assertEquals(count, 1);
    sub();
    child.data.set("a", 123);
    loro.commit();
    await one_ms();
    assertEquals(count, 1);
  });

  it("meta", () => {
    const root: LoroTreeNode = tree.createNode();
    root.data.set("a", 123);
    assertEquals(root.data.get("a"), 123);
  });
});

describe("loro tree node", () => {
  const loro = new LoroDoc();
  const tree = loro.getTree("root");
  tree.enableFractionalIndex(0);

  it("create", () => {
    const root = tree.createNode();
    const child = root.createNode();
    assertEquals(child.parent()!.id, root.id);
    const child2 = root.createNode();
    assertEquals(child.index(), 0);
    assertEquals(child2.index(), 1);
  });

  it("create with index", () => {
    const root = tree.createNode();
    const child = root.createNode();
    assertEquals(child.parent()!.id, root.id);
    const child2 = root.createNode(0);
    assertEquals(child.index(), 1);
    assertEquals(child2.index(), 0);
  });

  it("moveTo", () => {
    const root = tree.createNode();
    const child = root.createNode();
    const child2 = root.createNode();
    assertEquals(child2.parent()!.id, root.id);
    child2.move(child);
    assertEquals(child2.parent()!.id, child.id);
    assertEquals(child.children()![0].id, child2.id);
    expect(() => child2.move(child, 1)).toThrowError();
  });

  it("moveAfter", () => {
    const root = tree.createNode();
    const child = root.createNode();
    const child2 = root.createNode();
    assertEquals(child2.parent()!.id, root.id);
    child2.moveAfter(child);
    assertEquals(child2.parent()!.id, root.id);
    assertEquals(child.index(), 0);
    assertEquals(child2.index(), 1);
  });

  it("moveBefore", () => {
    const root = tree.createNode();
    const child = root.createNode();
    const child2 = root.createNode();
    assertEquals(child2.parent()!.id, root.id);
    child2.moveBefore(child);
    assertEquals(child2.parent()!.id, root.id);
    assertEquals(child.index(), 1);
    assertEquals(child2.index(), 0);
  });

  it("index", () => {
    const root = tree.createNode();
    const child = tree.createNode(root.id);
    const child2 = tree.createNode(root.id, 0);
    assertEquals(child.index(), 1);
    assertEquals(child2.index(), 0);
  });

  it("old parent", () => {
    const root = tree.createNode();
    const child = root.createNode();
    const child2 = root.createNode();
    loro.commit();
    const unsub = tree.subscribe((e) => {
      if (e.events[0].diff.type == "tree") {
        const diff = e.events[0].diff as TreeDiff;
        if (diff.diff[0].action == "move") {
          assertEquals(diff.diff[0].oldParent, root.id);
          assertEquals(diff.diff[0].oldIndex, 1);
        }
      }
    });
    child2.move(child);
    loro.commit();
    unsub()
    assertEquals(child2.parent()!.id, child.id);
  });
});

describe("LoroTree", () => {
  it("move", () => {
    const loro = new LoroDoc();
    const tree = loro.getTree("root");
    const root = tree.createNode();
    const child = tree.createNode(root.id);
    tree.move(child.id, root.id);
  })
})

function one_ms(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 1));
}
