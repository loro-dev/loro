import { describe, expect, test } from "vitest";

import {
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
});
