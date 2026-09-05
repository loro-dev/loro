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
    expect(list.getRangeValue(0, 4)).toStrictEqual(["a", "b", { k: "v" }, "d"]);
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
    expect(() => new LoroMovableList().getRangeDeepValueWithID(0, 1)).toThrow();
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
  // Walk every parsed JSON value. Identity is determined only by the sparse
  // position index, never by a scalar/object shape or a schema guess.
  function reattachContainerIds(result: {
    json: string;
    cids: readonly string[];
    containerPositions: Uint32Array;
  }): unknown {
    let position = 0;
    let next = 0;
    const walk = (value: any): unknown => {
      const cid =
        result.containerPositions[next] === position++
          ? result.cids[next++]
          : undefined;
      if (Array.isArray(value)) {
        for (let i = 0; i < value.length; i++) value[i] = walk(value[i]);
      } else if (value !== null && typeof value === "object") {
        for (const key of Object.keys(value)) value[key] = walk(value[key]);
      }
      return cid === undefined ? value : { cid, value };
    };
    const value = walk(JSON.parse(result.json));
    expect(next).toBe(result.cids.length);
    expect(next).toBe(result.containerPositions.length);
    return value;
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
    const result = doc.getDeepValueJsonWithIds();
    const { json, cids } = result;

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

    expect(reattachContainerIds(result)).toStrictEqual(
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
      const result = container.getDeepValueJsonWithIds();
      const { json, cids } = result;
      expect(cids[0], name).toBe(container.id);
      expect(JSON.parse(json), name).toStrictEqual(container.toJSON());
      // LoroCounter has no getDeepValueWithID(); its node shape is trivially
      // { cid: counter.id, value: number }
      if (name === "counter") {
        expect(cids, name).toStrictEqual([counter.id]);
        continue;
      }
      expect(reattachContainerIds(result), name).toStrictEqual(
        container.getDeepValueWithID(),
      );
    }
  });

  it("distinguishes scalar strings from Text containers at either position", () => {
    const results = ["a", "b"].map((textKey) => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      const map = doc.getMap("m");
      map.setContainer(textKey, new LoroText()).insert(0, "same");
      map.set(textKey === "a" ? "b" : "a", "same");
      const result = doc.getDeepValueJsonWithIds();
      expect(reattachContainerIds(result)).toStrictEqual(
        doc.getDeepValueWithID(),
      );
      return result;
    });
    expect(results[0].json).toBe(results[1].json);
    expect(results[0].cids).toStrictEqual(results[1].cids);
    expect([...results[0].containerPositions]).toStrictEqual([1, 2]);
    expect([...results[1].containerPositions]).toStrictEqual([1, 3]);
  });

  it("follows JS integer-key order at document and nested map levels", () => {
    const doc = new LoroDoc();
    for (const key of [
      "10",
      "2",
      "01",
      "0",
      "4294967294",
      "4294967295",
      "-1",
    ]) {
      const map = doc.getMap(key);
      map.setContainer("10", new LoroText()).insert(0, "ten");
      map.setContainer("2", new LoroText()).insert(0, "two");
      map.set("0", "plain");
    }
    const result = doc.getDeepValueJsonWithIds();
    expect(reattachContainerIds(result)).toStrictEqual(
      doc.getDeepValueWithID(),
    );
    expect(JSON.parse(result.json)).toStrictEqual(doc.toJSON());
  });

  it("preserves cid/value lookalikes and counts plain nested data and binary items", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("m");
    map.set("a", { cid: "cid:root-fake:Text", value: "ordinary data" });
    map.set("b", {
      "10": [1, "literal"],
      "2": { cid: "cid:root-fake:Map", value: {} },
    });
    map.set("c", new Uint8Array([3, 4, 5]));
    const text = map.setContainer("z", new LoroText());
    text.insert(0, "actual container");
    const result = doc.getDeepValueJsonWithIds();
    expect(result.cids).toStrictEqual([map.id, text.id]);
    expect(JSON.parse(result.json)).toStrictEqual(
      JSON.parse(doc.getDeepValueJson()),
    );
    // Binary's JSON representation is an array rather than toJSON's Uint8Array.
    const rebuilt = reattachContainerIds(result) as any;
    expect(rebuilt.m.value.a).toStrictEqual({
      cid: "cid:root-fake:Text",
      value: "ordinary data",
    });
    expect(rebuilt.m.value.z).toStrictEqual({
      cid: text.id,
      value: "actual container",
    });
  });

  it("keeps __proto__ as an own JSON property without changing prototypes", () => {
    const doc = new LoroDoc();
    doc.getText("__proto__").insert(0, "data");
    const result = doc.getDeepValueJsonWithIds();
    const parsed = JSON.parse(result.json);
    const restored = reattachContainerIds(result) as Record<string, unknown>;
    expect(Object.getPrototypeOf(parsed)).toBe(Object.prototype);
    expect(Object.getPrototypeOf(restored)).toBe(Object.prototype);
    expect(Object.prototype.hasOwnProperty.call(restored, "__proto__")).toBe(
      true,
    );
    expect(restored.__proto__).toStrictEqual({
      cid: "cid:root-__proto__:Text",
      value: "data",
    });
  });

  it("preserves mixed scalar/container arrays and plain empty values", () => {
    const doc = new LoroDoc();
    const list = doc.getList("list");
    list.push("same");
    list.pushContainer(new LoroText()).insert(0, "same");
    list.push({});
    list.pushContainer(new LoroMap());
    list.push([]);
    list.pushContainer(new LoroList());
    list.push(null);
    list.push(1.5);
    list.pushContainer(new LoroCounter()).increment(1.5);
    const result = list.getDeepValueJsonWithIds();
    expect(reattachContainerIds(result)).toStrictEqual(
      list.getDeepValueWithID(),
    );
    expect(result.containerPositions[0]).toBe(0);
  });

  it("empty doc yields {} and no cids", () => {
    const doc = new LoroDoc();
    const result = doc.getDeepValueJsonWithIds();
    const { json, cids } = result;
    expect(json).toBe("{}");
    expect(cids).toStrictEqual([]);
  });
});
