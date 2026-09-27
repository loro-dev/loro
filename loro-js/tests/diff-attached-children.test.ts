import { describe, expect, test } from "vitest";

import {
  UndoManager,
  LoroDoc,
  LoroList,
  LoroMap,
  LoroMovableList,
  LoroText,
  type ContainerID,
  type Frontiers,
} from "../src/index";

/**
 * `diff(from, to)` carries the whole state of a container that is unreachable
 * at `from` and reachable at `to`, and only its own ops otherwise. `revertTo`
 * applies such a diff. Expected values match Rust's `revert_to`, except for
 * mergeable children, where Rust currently duplicates the resurfaced content.
 */
function revertAfter(build: (doc: LoroDoc) => void, edit: (doc: LoroDoc) => void) {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  build(doc);
  doc.commit();
  const target = doc.frontiers();
  const expected = doc.toJSON();
  edit(doc);
  doc.commit();
  const latest = doc.frontiers();
  const diff = doc.diff(latest, target);
  doc.revertTo(target);
  doc.commit();
  return { doc, diff, expected, target, latest };
}

const containerIds = (diff: [ContainerID, unknown][]): ContainerID[] =>
  diff.map(([id]) => id);

describe("diff and revertTo for containers attached by the range", () => {
  test("does not repeat the content of a moved movable-list child", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const list = doc.getMovableList("l");
    const text = list.insertContainer(0, new LoroText());
    text.insert(0, "xxxx");
    list.push("tail");
    doc.commit();
    const before = doc.frontiers();
    list.move(0, 1);
    doc.commit();

    const diff = doc.diff(doc.frontiers(), before);
    expect(containerIds(diff)).toEqual([list.id]);

    // Applied to a replica at the latest version, the child stays intact.
    const replica = doc.fork();
    replica.setDetachedEditing(true);
    replica.applyDiff(diff);
    expect(replica.toJSON()).toEqual({ l: ["xxxx", "tail"] });

    doc.revertTo(before);
    expect(doc.toJSON()).toEqual({ l: ["xxxx", "tail"] });
  });

  test("restores mergeable children once", () => {
    const { doc } = revertAfter(
      (doc) => {
        const map = doc.getMap("m");
        map.ensureMergeableText("text").insert(0, "hello");
        map.ensureMergeableCounter("counter").increment(7);
        map.ensureMergeableList("list").push("keep");
        map.ensureMergeableMovableList("movable").push("keep");
        map.ensureMergeableMap("map").set("k", "v");
      },
      (doc) => {
        for (const key of ["text", "counter", "list", "movable", "map"]) {
          doc.getMap("m").delete(key);
        }
      },
    );
    expect(doc.toJSON()).toEqual({
      m: {
        text: "hello",
        counter: 7,
        list: ["keep"],
        movable: ["keep"],
        map: { k: "v" },
      },
    });
  });

  test("restores the whole state of a reattached child that also changed", () => {
    for (const parent of ["map", "list", "movable"] as const) {
      const { doc, expected } = revertAfter(
        (doc) => {
          const child =
            parent === "map"
              ? doc.getMap("m").setContainer("s", new LoroMap())
              : parent === "list"
                ? doc.getList("m").insertContainer(0, new LoroMap())
                : doc.getMovableList("m").insertContainer(0, new LoroMap());
          child.set("a", 1);
          child.set("b", 2);
        },
        (doc) => {
          const container = doc.getContainerById(
            parent === "map"
              ? "cid:root-m:Map"
              : parent === "list"
                ? "cid:root-m:List"
                : "cid:root-m:MovableList",
          ) as LoroMap | LoroList | LoroMovableList;
          const child = (
            container instanceof LoroMap ? container.get("s") : container.get(0)
          ) as LoroMap;
          child.set("a", 3);
          if (container instanceof LoroMap) container.set("s", "replaced");
          else container.delete(0, 1);
        },
      );
      expect({ parent, value: doc.toJSON() }).toEqual({ parent, value: expected });
    }
  });

  test("orders a reattached parent before its changed child", () => {
    const { doc, diff, expected } = revertAfter(
      (doc) => {
        const s = doc.getMap("m").setContainer("s", new LoroMap());
        const g = s.setContainer("g", new LoroMap());
        g.set("a", 1);
        g.set("b", 2);
        s.set("keep", true);
      },
      (doc) => {
        const s = doc.getMap("m").get("s") as LoroMap;
        (s.get("g") as LoroMap).set("a", 3);
        doc.getMap("m").delete("s");
      },
    );
    expect(containerIds(diff)).toEqual(["cid:root-m:Map", "cid:0@1:Map", "cid:1@1:Map"]);
    expect(doc.toJSON()).toEqual(expected);
  });

  test("restores the metadata of a revived tree node", () => {
    const { doc, expected } = revertAfter(
      (doc) => {
        const node = doc.getTree("tree").createNode();
        node.data.set("name", "keep");
        node.data.setContainer("text", new LoroText()).insert(0, "hello");
      },
      (doc) => {
        const tree = doc.getTree("tree");
        tree.delete(tree.roots()[0]!.id);
      },
    );
    const [node] = (doc.toJSON() as { tree: { meta: unknown }[] }).tree;
    expect(node!.meta).toEqual({ name: "keep", text: "hello" });
    expect((expected as { tree: { meta: unknown }[] }).tree[0]!.meta).toEqual(node!.meta);
  });

  test("keeps a reattached child's nested containers", () => {
    const { doc, expected } = revertAfter(
      (doc) => {
        const s = doc.getMap("map").setContainer("sub", new LoroMap());
        s.set("k", "v");
        s.setContainer("text", new LoroText()).insert(0, "hi");
        doc.getList("list").insertContainer(0, new LoroMap()).set("x", 1);
      },
      (doc) => {
        doc.getMap("map").set("sub", "not a map");
        doc.getList("list").delete(0, 1);
      },
    );
    expect(doc.toJSON()).toEqual(expected);
  });

  test("applies to a replica that is still at the start version", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const map = doc.getMap("m");
    const child = map.setContainer("s", new LoroMap());
    child.set("a", 1);
    doc.commit();
    const start: Frontiers = doc.frontiers();
    map.delete("s");
    doc.commit();
    const deleted = doc.frontiers();
    const diff = doc.diff(deleted, start);
    const replica = doc.forkAt(deleted);
    replica.setDetachedEditing(true);
    replica.applyDiff(diff);
    expect(replica.toJSON()).toEqual({ m: { s: { a: 1 } } });
  });

  test("applyDiff moves an existing child with one undoable move", () => {
    for (const toEnd of [true, false]) {
      const doc = new LoroDoc();
      doc.setPeerId(1);
      const list = doc.getMovableList("l");
      const text = list.insertContainer(0, new LoroText());
      text.insert(0, "xxxx");
      for (let index = 0; index < 5; index += 1) list.push(index);
      if (!toEnd) list.move(0, 5);
      doc.commit();
      const before = doc.toJSON();
      const opsBefore = doc.opCount();
      const undo = new UndoManager(doc, { mergeInterval: 0 });
      doc.applyDiff([
        [
          list.id,
          {
            type: "list",
            diff: toEnd
              ? [{ delete: 1 }, { retain: 5 }, { insert: [`🦜:${text.id}`] }]
              : [{ insert: [`🦜:${text.id}`] }, { retain: 5 }, { delete: 1 }],
          },
        ],
      ]);
      doc.commit();
      const after = doc.toJSON();
      expect(after).toEqual({
        l: toEnd ? [0, 1, 2, 3, 4, "xxxx"] : ["xxxx", 0, 1, 2, 3, 4],
      });
      // Rust's MovableList apply_delta emits one move for the pair.
      expect(doc.opCount() - opsBefore).toBe(1);
      expect(undo.undo()).toBe(true);
      expect(doc.toJSON()).toEqual(before);
      expect(undo.redo()).toBe(true);
      expect(doc.toJSON()).toEqual(after);
    }
  });

  test("an applied move merges with a concurrent move like Rust's", () => {
    const base = new LoroDoc();
    base.setPeerId(1);
    const list = base.getMovableList("l");
    const text = list.insertContainer(0, new LoroText());
    text.insert(0, "text");
    for (const value of ["A", "B", "C"]) list.push(value);
    base.commit();
    const version = base.oplogVersion();

    const mover = base.fork();
    mover.setPeerId(2);
    mover.applyDiff([
      [
        list.id,
        {
          type: "list",
          diff: [{ delete: 1 }, { retain: 3 }, { insert: [`🦜:${text.id}`] }],
        },
      ],
    ]);
    mover.commit();
    const concurrent = base.fork();
    concurrent.setPeerId(3);
    concurrent.getMovableList("l").move(3, 0);
    concurrent.commit();

    const receiver = base.fork();
    receiver.import(mover.export({ mode: "update", from: version }));
    receiver.import(concurrent.export({ mode: "update", from: version }));
    expect(receiver.toJSON()).toEqual({ l: ["C", "A", "B", "text"] });
  });

  test("restores a revived parent's whole subtree", () => {
    const { doc, diff } = revertAfter(
      (doc) => {
        const tree = doc.getTree("tree");
        const parent = tree.createNode();
        const child = parent.createNode();
        child.data.set("k", "v");
        child.createNode().data.setContainer("t", new LoroText()).insert(0, "deep");
      },
      (doc) => doc.getTree("tree").delete("0@1"),
    );
    expect(
      diff
        .filter(([id]) => id === "cid:root-tree:Tree")
        .flatMap(([, value]) => (value as { diff: { action: string }[] }).diff)
        .map((item) => item.action),
    ).toEqual(["create", "create", "create"]);
    const [root] = (doc.toJSON() as { tree: TreeJson[] }).tree;
    expect(root!.children[0]!.meta).toEqual({ k: "v" });
    expect(root!.children[0]!.children[0]!.meta).toEqual({ t: "deep" });
  });

  // With a shallow history, the same move must stay a move even though the
  // create op was trimmed (#treeNodeAliveAt falls back to the root state). The
  // shallow retreat that needs is fixed separately (loro-dev/loro#1127), whose
  // shallow-root-map-winner tests cover the combination.
  test("reports a tree move of a node that stays alive as a move", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const tree = doc.getTree("tree");
    const moved = tree.createNode();
    const target = tree.createNode();
    moved.data.set("x", "root");
    doc.commit();
    const root = doc.frontiers();
    tree.move(moved.id, target.id);
    doc.commit();
    const diff = doc.diff(root, doc.frontiers());
    const items = diff.find(([id]) => id === tree.id)![1] as {
      diff: { action: string }[];
    };
    expect(items.diff.map((item) => item.action)).toEqual(["move"]);
    const replica = doc.forkAt(root);
    replica.setDetachedEditing(true);
    replica.applyDiff(diff);
    expect(replica.toJSON()).toEqual(doc.toJSON());
  });

  test("recreates a node moved out of a deleted ancestor", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const tree = doc.getTree("tree");
    const parent = tree.createNode();
    const child = parent.createNode();
    child.data.setContainer("text", new LoroText()).insert(0, "hello");
    child.data.ensureMergeableText("merge").insert(0, "keep");
    doc.commit();
    tree.delete(parent.id);
    doc.commit();
    const from = doc.frontiers();
    tree.move(child.id);
    doc.commit();
    const diff = doc.diff(from, doc.frontiers());
    const treeItems = diff.find(([id]) => id === tree.id)![1] as {
      diff: { action: string }[];
    };
    expect(treeItems.diff.map((item) => item.action)).toEqual(["create"]);

    const replica = doc.forkAt(from);
    replica.setDetachedEditing(true);
    replica.applyDiff(diff);
    const [node] = (replica.toJSON() as { tree: TreeJson[] }).tree;
    expect(node!.meta).toEqual({ text: "hello", merge: "keep" });
  });
});

interface TreeJson {
  readonly meta: unknown;
  readonly children: TreeJson[];
}
