import { describe, expect, it } from "vitest";
import {
  LoroDoc,
  LoroList,
  LoroMap,
  LoroMovableList,
  LoroText,
  LoroTree,
  ValueWithContainerID,
} from "../bundler/index";

describe("getDeepValueWithID", () => {
  it("doc-level cid equals the container id string", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.set("key", "value");
    const child = map.setContainer("child", new LoroList());
    child.insert(0, "item");
    doc.commit();

    const value = doc.getDeepValueWithID() as Record<
      string,
      ValueWithContainerID
    >;
    expect(value.map.cid).toBe(map.id);
    expect(value.map.cid).toBe("cid:root-map:Map");
    const childNode = (value.map.value as Record<string, ValueWithContainerID>)
      .child;
    expect(childNode.cid).toBe(child.id);
    expect(childNode.cid.startsWith("cid:")).toBe(true);
    expect(childNode.cid.includes("idx:")).toBe(false);
    expect(childNode.value).toStrictEqual(["item"]);
  });

  it("per-container nodes match the doc-level node shape", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.set("foo", "bar");
    const text = map.setContainer("text", new LoroText());
    text.insert(0, "Hello");
    doc.commit();

    expect(map.getDeepValueWithID()).toStrictEqual({
      cid: map.id,
      value: {
        foo: "bar",
        text: { cid: text.id, value: "Hello" },
      },
    });
    expect(text.getDeepValueWithID()).toStrictEqual({
      cid: text.id,
      value: "Hello",
    });
  });

  it("list and movable list nodes", () => {
    const doc = new LoroDoc();
    const list = doc.getList("list");
    list.insert(0, 100);
    const text = list.insertContainer(1, new LoroText());
    text.insert(0, "Hello");

    const movable = doc.getMovableList("movable");
    movable.insert(0, "a");
    const sub = movable.insertContainer(1, new LoroList());
    sub.insert(0, "x");
    doc.commit();

    expect(list.getDeepValueWithID()).toStrictEqual({
      cid: list.id,
      value: [100, { cid: text.id, value: "Hello" }],
    });
    expect(movable.getDeepValueWithID()).toStrictEqual({
      cid: movable.id,
      value: ["a", { cid: sub.id, value: ["x"] }],
    });
  });

  it("tree node", () => {
    const doc = new LoroDoc();
    const tree = doc.getTree("tree");
    const root = tree.createNode();
    root.data.set("name", "root");
    doc.commit();

    const node = tree.getDeepValueWithID();
    expect(node.cid).toBe(tree.id);
    const nodes = node.value as {
      id: string;
      meta: Record<string, unknown>;
      children: unknown[];
    }[];
    expect(nodes).toHaveLength(1);
    expect(nodes[0].id).toBe(root.id);
    expect(nodes[0].meta).toStrictEqual({ name: "root" });
    expect(nodes[0].children).toStrictEqual([]);
  });

  it("throws on detached containers instead of trapping", () => {
    expect(() => new LoroMap().getDeepValueWithID()).toThrow();
    expect(() => new LoroList().getDeepValueWithID()).toThrow();
    expect(() => new LoroMovableList().getDeepValueWithID()).toThrow();
    expect(() => new LoroTree().getDeepValueWithID()).toThrow();
    expect(() => new LoroText().getDeepValueWithID()).toThrow();
  });
});

describe("range deep reads", () => {
  const setup = () => {
    const doc = new LoroDoc();
    const list = doc.getList("list");
    list.insert(0, "a");
    list.insert(1, "b");
    const child = list.insertContainer(2, new LoroMap());
    child.set("k", "v");
    list.insert(3, "d");
    doc.commit();
    return { doc, list, child };
  };

  it("getRangeDeepValueWithID resolves containers to { cid, value } nodes", () => {
    const { list, child } = setup();
    expect(list.getRangeDeepValueWithID(0, 4)).toStrictEqual([
      "a",
      "b",
      { cid: child.id, value: { k: "v" } },
      "d",
    ]);
    expect(list.getRangeDeepValueWithID(2, 3)).toStrictEqual([
      { cid: child.id, value: { k: "v" } },
    ]);
  });

  it("getRangeValue resolves containers to plain deep values", () => {
    const { list } = setup();
    expect(list.getRangeValue(1, 3)).toStrictEqual(["b", { k: "v" }]);
    expect(list.getRangeValue(0, 4)).toStrictEqual([
      "a",
      "b",
      { k: "v" },
      "d",
    ]);
  });

  it("clamps out-of-range bounds and returns [] for empty ranges", () => {
    const { list } = setup();
    // end > len clamps to len
    expect(list.getRangeValue(3, 100)).toStrictEqual(["d"]);
    // negative start clamps to 0
    expect(list.getRangeValue(-1, 1)).toStrictEqual(["a"]);
    expect(list.getRangeDeepValueWithID(-5, 2)).toStrictEqual(["a", "b"]);
    // start >= len, empty, and inverted ranges are empty
    expect(list.getRangeValue(100, 200)).toStrictEqual([]);
    expect(list.getRangeValue(2, 2)).toStrictEqual([]);
    expect(list.getRangeValue(3, 1)).toStrictEqual([]);
  });

  it("nested containers inside the range carry their own { cid, value }", () => {
    const doc = new LoroDoc();
    const list = doc.getList("list");
    const outer = list.insertContainer(0, new LoroList());
    const inner = outer.insertContainer(0, new LoroText());
    inner.insert(0, "deep");
    doc.commit();

    expect(list.getRangeDeepValueWithID(0, 1)).toStrictEqual([
      {
        cid: outer.id,
        value: [{ cid: inner.id, value: "deep" }],
      },
    ]);
  });

  it("works on movable lists", () => {
    const doc = new LoroDoc();
    const list = doc.getMovableList("list");
    list.insert(0, "a");
    const child = list.insertContainer(1, new LoroText());
    child.insert(0, "hi");
    list.insert(2, "b");
    doc.commit();

    expect(list.getRangeDeepValueWithID(0, 3)).toStrictEqual([
      "a",
      { cid: child.id, value: "hi" },
      "b",
    ]);
    expect(list.getRangeValue(1, 2)).toStrictEqual(["hi"]);
    expect(list.getRangeValue(0, 0)).toStrictEqual([]);
  });

  it("throws on detached lists", () => {
    expect(() => new LoroList().getRangeValue(0, 1)).toThrow();
    expect(() => new LoroList().getRangeDeepValueWithID(0, 1)).toThrow();
    expect(() => new LoroMovableList().getRangeValue(0, 1)).toThrow();
    expect(() =>
      new LoroMovableList().getRangeDeepValueWithID(0, 1),
    ).toThrow();
  });
});
