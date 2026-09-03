import { describe, expect, it } from "vitest";
import {
  LoroCounter,
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

describe("getDeepValueJson", () => {
  const setupAll = () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.set("flag", true);
    map.set("n", 42);
    const text = map.setContainer("text", new LoroText());
    text.insert(0, "Hello");
    const list = doc.getList("list");
    list.insert(0, "a");
    const sub = list.insertContainer(1, new LoroMap());
    sub.set("k", "v");
    const movable = doc.getMovableList("movable");
    movable.insert(0, "x");
    const tree = doc.getTree("tree");
    const root = tree.createNode();
    root.data.set("name", "root");
    const child = root.createNode();
    child.data.set("name", "child");
    const counter = doc.getCounter("counter");
    counter.increment(2.5);
    doc.commit();
    return { doc, map, text, list, sub, movable, tree, counter };
  };

  it("JSON.parse matches toJSON for a doc with every container type", () => {
    const { doc, map, text, list, movable, tree, counter } = setupAll();
    expect(JSON.parse(doc.getDeepValueJson())).toStrictEqual(doc.toJSON());
    expect(JSON.parse(map.getDeepValueJson())).toStrictEqual(map.toJSON());
    expect(JSON.parse(text.getDeepValueJson())).toStrictEqual(text.toJSON());
    expect(JSON.parse(list.getDeepValueJson())).toStrictEqual(list.toJSON());
    expect(JSON.parse(movable.getDeepValueJson())).toStrictEqual(
      movable.toJSON(),
    );
    expect(JSON.parse(tree.getDeepValueJson())).toStrictEqual(tree.toJSON());
    expect(JSON.parse(counter.getDeepValueJson())).toStrictEqual(
      counter.toJSON(),
    );
  });

  it("empty doc serializes to {}", () => {
    const doc = new LoroDoc();
    expect(doc.getDeepValueJson()).toBe("{}");
  });

  it("throws on detached containers instead of trapping", () => {
    expect(() => new LoroMap().getDeepValueJson()).toThrow();
    expect(() => new LoroList().getDeepValueJson()).toThrow();
    expect(() => new LoroMovableList().getDeepValueJson()).toThrow();
    expect(() => new LoroTree().getDeepValueJson()).toThrow();
    expect(() => new LoroText().getDeepValueJson()).toThrow();
    expect(() => new LoroMap().getDeepValueJsonWithIds()).toThrow();
    expect(() => new LoroList().getDeepValueJsonWithIds()).toThrow();
    expect(() => new LoroMovableList().getDeepValueJsonWithIds()).toThrow();
    expect(() => new LoroTree().getDeepValueJsonWithIds()).toThrow();
    expect(() => new LoroText().getDeepValueJsonWithIds()).toThrow();
    // Counter mirrors toJSON(): it also works on a detached counter
    expect(new LoroCounter().getDeepValueJson()).toBe("0.0");
  });
});

describe("getDeepValueJsonWithIds", () => {
  type ContainerTypeName =
    | "Text"
    | "Map"
    | "List"
    | "MovableList"
    | "Tree"
    | "Counter";

  const containerTypeOf = (cid: string): ContainerTypeName =>
    cid.slice(cid.lastIndexOf(":") + 1) as ContainerTypeName;

  /**
   * Rebuild the getDeepValueWithID() shape from the parsed `json` and the
   * positional `cids` stream, walking against the known Loro grammar:
   * root object -> every value is a container; Map -> object entries;
   * List/MovableList/Tree -> arrays; Text -> string; Counter -> number.
   */
  function reattachContainerIds(
    json: unknown,
    cids: readonly string[],
    isRoot: boolean,
  ): unknown {
    let i = 0;
    const shapeMatches = (type: ContainerTypeName, value: unknown): boolean => {
      switch (type) {
        case "Text":
          return typeof value === "string";
        case "Counter":
          return typeof value === "number";
        case "Map":
          return (
            typeof value === "object" && value !== null && !Array.isArray(value)
          );
        case "List":
        case "MovableList":
        case "Tree":
          return Array.isArray(value);
      }
    };
    const walkContainerValue = (
      type: ContainerTypeName,
      value: any,
    ): unknown => {
      switch (type) {
        case "Text":
        case "Counter":
          return value;
        case "Map": {
          const out: Record<string, unknown> = {};
          for (const [k, v] of Object.entries(value)) out[k] = attach(v);
          return out;
        }
        case "List":
        case "MovableList":
          return value.map(attach);
        case "Tree":
          // Tree node meta maps are plain deep values; no container ids inside
          return value;
      }
    };
    // Consume the next cid and wrap `value` as a { cid, value } node.
    const attachForced = (value: unknown): ValueWithContainerID => {
      const cid = cids[i++];
      return {
        cid: cid as ValueWithContainerID["cid"],
        value: walkContainerValue(containerTypeOf(cid), value) as never,
      };
    };
    // Wrap `value` only if the next pending cid's type matches its shape;
    // otherwise it is a plain value and is kept as-is.
    const attach = (value: unknown): unknown => {
      const cid = cids[i];
      if (cid === undefined || !shapeMatches(containerTypeOf(cid), value)) {
        return value;
      }
      return attachForced(value);
    };

    if (!isRoot) return attachForced(json);
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(json as Record<string, unknown>)) {
      out[k] = attachForced(v);
    }
    return out;
  }

  const setupAll = () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.set("flag", true);
    map.set("n", 42);
    const text = map.setContainer("text", new LoroText());
    text.insert(0, "Hello");
    const list = doc.getList("list");
    list.insert(0, "a");
    const sub = list.insertContainer(1, new LoroMap());
    sub.set("k", "v");
    const movable = doc.getMovableList("movable");
    movable.insert(0, "x");
    const tree = doc.getTree("tree");
    const root = tree.createNode();
    root.data.set("name", "root");
    const child = root.createNode();
    child.data.set("name", "child");
    const counter = doc.getCounter("counter");
    counter.increment(2.5);
    doc.commit();
    return { doc, map, text, list, sub, movable, tree, counter };
  };

  it("doc-level: json + cids reconstruct getDeepValueWithID", () => {
    const { doc, map, text, list, sub, movable, tree, counter } = setupAll();
    const { json, cids } = doc.getDeepValueJsonWithIds();

    // json parses to the same content as toJSON()
    expect(JSON.parse(json)).toStrictEqual(doc.toJSON());

    // cids are the pre-order DFS of the serialized tree (root keys are
    // serialized in sorted order: counter, list, map, movable, tree)
    expect(cids).toStrictEqual([
      counter.id,
      list.id,
      sub.id,
      map.id,
      text.id,
      movable.id,
      tree.id,
    ]);

    expect(reattachContainerIds(JSON.parse(json), cids, true)).toStrictEqual(
      doc.getDeepValueWithID(),
    );
  });

  it("per-container: cids[0] is the container id and the walk round-trips", () => {
    const { map, text, list, movable, tree, counter } = setupAll();
    const containers = [
      ["map", map],
      ["text", text],
      ["list", list],
      ["movable", movable],
      ["tree", tree],
      ["counter", counter],
    ] as const;
    for (const [name, container] of containers) {
      const { json, cids } = container.getDeepValueJsonWithIds();
      expect(cids[0], name).toBe(container.id);
      expect(JSON.parse(json), name).toStrictEqual(container.toJSON());
      // LoroCounter has no getDeepValueWithID(); its node shape is trivially
      // { cid: counter.id, value: number }
      if (name === "counter") {
        expect(cids, name).toStrictEqual([counter.id]);
        continue;
      }
      expect(
        reattachContainerIds(JSON.parse(json), cids, false),
        name,
      ).toStrictEqual(container.getDeepValueWithID());
    }
  });

  it("empty doc yields {} and no cids", () => {
    const doc = new LoroDoc();
    const { json, cids } = doc.getDeepValueJsonWithIds();
    expect(json).toBe("{}");
    expect(cids).toStrictEqual([]);
  });
});
