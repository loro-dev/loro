import { describe, expect, it } from "vitest";
import { LoroDoc, LoroTree, TreeDiffItem, TreeID } from "../bundler/index";

// Tree events must turn the tree before an import into the tree after it,
// including the nodes below a node revived from a deleted subtree
// (loro-dev/loro#1157). See context/tree-events.md.

const ROOT = "root";

/** Children lists of the alive tree, rebuilt only from events. */
class Mirror {
  children = new Map<string, TreeID[]>();

  static fromTree(tree: LoroTree): Mirror {
    const m = new Mirror();
    const walk = (key: string, ids: TreeID[]) => {
      if (ids.length > 0) m.children.set(key, ids);
      for (const id of ids) {
        walk(
          id,
          tree
            .getNodeByID(id)!
            .children()
            ?.map((n) => n.id) ?? [],
        );
      }
    };
    walk(
      ROOT,
      tree.roots().map((n) => n.id),
    );
    return m;
  }

  alive(id: TreeID): boolean {
    for (const ids of this.children.values()) {
      if (ids.includes(id)) return true;
    }
    return false;
  }

  remove(target: TreeID, parent: TreeID | undefined, index: number) {
    const key = parent ?? ROOT;
    const ids = this.children.get(key);
    expect(ids?.[index], `${target}: old index`).toBe(target);
    ids!.splice(index, 1);
    if (ids!.length === 0) this.children.delete(key);
  }

  insert(target: TreeID, parent: TreeID | undefined, index: number) {
    if (parent !== undefined) {
      expect(this.alive(parent), `${target}: parent ${parent} alive`).toBe(
        true,
      );
    }
    const key = parent ?? ROOT;
    const ids = this.children.get(key) ?? [];
    expect(index).toBeLessThanOrEqual(ids.length);
    ids.splice(index, 0, target);
    this.children.set(key, ids);
  }

  apply(item: TreeDiffItem) {
    if (item.action === "create") {
      expect(this.alive(item.target), `${item.target}: created twice`).toBe(
        false,
      );
      this.insert(item.target, item.parent, item.index);
    } else if (item.action === "move") {
      this.remove(item.target, item.oldParent, item.oldIndex);
      this.insert(item.target, item.parent, item.index);
    } else {
      this.remove(item.target, item.oldParent, item.oldIndex);
      // The whole subtree goes with it.
      const stack: string[] = [item.target];
      while (stack.length > 0) {
        const key = stack.pop()!;
        const ids = this.children.get(key);
        if (ids) {
          this.children.delete(key);
          stack.push(...ids);
        }
      }
    }
  }
}

/** Imports `updates` into a copy of `base` and checks the events. */
function checkImportEvents(base: LoroDoc, updates: Uint8Array) {
  const doc = new LoroDoc();
  doc.import(base.export({ mode: "update" }));
  const tree = doc.getTree("tree");
  const mirror = Mirror.fromTree(tree);
  const items: TreeDiffItem[] = [];
  doc.subscribe((e) => {
    for (const ev of e.events) {
      if (ev.diff.type === "tree") items.push(...ev.diff.diff);
    }
  });
  doc.import(updates);
  for (const item of items) mirror.apply(item);
  expect(mirror.children).toEqual(Mirror.fromTree(tree).children);
}

function docWithSubtree() {
  const doc = new LoroDoc();
  doc.setPeerId(1n);
  const tree = doc.getTree("tree");
  const p = tree.createNode();
  const x = p.createNode();
  const y1 = x.createNode();
  x.createNode();
  y1.createNode();
  doc.commit();
  return { doc, tree, p, x, y1 };
}

describe("tree events on import", () => {
  it("create every node below a node moved out of a deleted subtree", () => {
    const { doc, tree, p, x } = docWithSubtree();
    const base = doc.fork();
    const v0 = doc.oplogVersion();
    tree.delete(p.id);
    doc.commit();
    tree.move(x.id, undefined);
    doc.commit();
    checkImportEvents(base, doc.export({ mode: "update", from: v0 }));
  });

  it("delete a node moved under a deleted one", () => {
    const { doc, tree, p, y1 } = docWithSubtree();
    const q = tree.createNode();
    q.createNode();
    doc.commit();
    const base = doc.fork();
    const v0 = doc.oplogVersion();
    tree.delete(p.id);
    tree.move(q.id, y1.id);
    doc.commit();
    checkImportEvents(base, doc.export({ mode: "update", from: v0 }));
  });
});
