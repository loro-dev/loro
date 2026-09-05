import { describe, expect, it } from "vitest";
import {
  LoroDoc,
  LoroMap,
  LoroText,
  LoroList,
  LoroMovableList,
  LoroTree,
  LoroCounter,
  type ContainerNode,
  type ContainerTreeNode,
} from "../bundler/index";

function project(node: ContainerTreeNode): unknown {
  switch (node.type) {
    case "Value":
      return node.value;
    case "Map":
      return Object.fromEntries(
        Object.entries(node.value).map(([k, v]) => [k, project(v)]),
      );
    case "List":
    case "MovableList":
      return node.value.map(project);
    case "Tree": {
      const visit = (nodes: typeof node.value): unknown[] =>
        nodes.map((n) => ({
          ...n,
          meta: project(n.meta),
          children: visit(n.children),
        }));
      return visit(node.value);
    }
    default:
      return node.value;
  }
}
function ids(node: ContainerTreeNode): string[] {
  if (node.type === "Value") return [];
  if (node.type === "Map")
    return [node.cid, ...Object.values(node.value).flatMap(ids)];
  if (node.type === "List" || node.type === "MovableList")
    return [node.cid, ...node.value.flatMap(ids)];
  return [node.cid];
}

describe("toContainerTree", () => {
  it("distinguishes containers from opaque values and preserves exact IDs", () => {
    const d = new LoroDoc();
    d.setPeerId("18446744073709551614");
    const m = d.getMap("root");
    const fake = {
      type: "Map",
      cid: "cid:root-fake:Map",
      value: { nested: [null, true, "🙂"] },
    };
    m.set("fake", fake);
    m.set("__proto__", { safe: true });
    m.set("bytes", new Uint8Array([0, 255, 128]));
    m.set("number", Infinity);
    m.set("nan", NaN);
    const l = m.setContainer("list", new LoroMovableList());
    const t = l.pushContainer(new LoroText());
    t.insert(0, "\uFEFFhello中🙂");
    const merged = m.ensureMergeableMap("merged");
    merged.set("ok", true);
    const read = d.toContainerTree();
    expect(project(read.root)).toStrictEqual({
      ...m.toJSON(),
      ["__proto__"]: { safe: true },
    });
    expect(ids(read.root).sort()).toEqual([m.id, l.id, t.id, merged.id].sort());
    const root = read.root as Extract<ContainerNode, { type: "Map" }>;
    expect(root.value.fake).toEqual({ type: "Value", value: fake });
    expect(Object.prototype.hasOwnProperty.call(root.value, "__proto__")).toBe(
      true,
    );
    expect(Object.getPrototypeOf(root.value)).toBe(Object.prototype);
    expect(m.toContainerTree()).toStrictEqual(root);
    const bytes = root.value.bytes as { type: "Value"; value: Uint8Array };
    bytes.value[0] = 42;
    expect(m.get("bytes")).toEqual(new Uint8Array([0, 255, 128]));
    d.free();
    expect(bytes.value[1]).toBe(255);
  });

  it("includes Tree metadata IDs and rich text formatting", () => {
    const d = new LoroDoc();
    const tree = d.getTree("tree");
    const node = tree.createNode();
    const text = node.data.setContainer("body", new LoroText());
    text.insert(0, "hello");
    text.mark({ start: 0, end: 5 }, "bold", true);
    node.createNode().data.set("title", "child");
    const plain = d.toContainerTree().tree;
    expect(project(plain)).toEqual(d.toJSON().tree);
    const rich = tree.toContainerTree({ text: "delta" });
    if (rich.type !== "Tree") throw new Error("expected Tree");
    expect(rich.value[0].meta.cid).toBe(node.data.id);
    expect(rich.value[0].meta.value.body).toEqual({
      type: "Text",
      cid: text.id,
      value: text.toDelta(),
    });
  });

  it("reads only the selected list subtrees and clamps ranges", () => {
    for (const list of [new LoroList(), new LoroMovableList()]) {
      const d = new LoroDoc();
      const l = d.getMap("root").setContainer("list", list);
      l.push(1);
      l.pushContainer(new LoroMap()).set("x", 2);
      l.push("end");
      const full = l.toContainerTree();
      if (full.type !== "List" && full.type !== "MovableList")
        throw new Error("expected list");
      expect(l.toContainerTreeSlice(1, 2)).toEqual({
        cid: l.id,
        start: 1,
        totalLength: 3,
        items: full.value.slice(1, 2),
      });
      for (const range of [
        { start: 3, end: 99 },
        { start: 2, end: 1 },
        { start: 99, end: 100 },
      ]) {
        expect(l.toContainerTreeSlice(range.start, range.end)).toEqual({
          cid: l.id,
          start: Math.min(range.start, 3),
          totalLength: 3,
          items: [],
        });
      }
    }
  });

  it("rejects invalid inputs and remains usable", () => {
    const d = new LoroDoc();
    d.getMap("root").set("ok", true);
    const read = d.toContainerTree.bind(d) as (o: unknown) => unknown;
    for (const options of [
      { container: "bad" },
      { container: "cid:99@88:Map" },
      { text: "html" },
      { range: { start: 0, end: 1 } },
      { container: "cid:root-root:Map", range: { start: 0, end: 1 } },
      { container: "cid:root-l:List", range: { start: -1, end: 1 } },
    ]) {
      expect(() => read(options)).toThrow();
      expect(project(d.toContainerTree().root)).toEqual({ ok: true });
    }
  });

  it("does not invoke inherited setters or commit pending changes", () => {
    const d = new LoroDoc();
    const m = d.getMap("root");
    m.set("__read_state_probe", "data");
    m.set("array", [1, 2]);
    let calls = 0;
    const vv = d.version().encode();
    Object.defineProperty(Object.prototype, "__read_state_probe", {
      configurable: true,
      set() {
        calls++;
      },
    });
    let result: ReturnType<LoroDoc["toContainerTree"]>;
    try {
      result = d.toContainerTree();
    } finally {
      delete (Object.prototype as Record<string, unknown>).__read_state_probe;
    }
    expect(calls).toBe(0);
    expect(d.version().encode()).toEqual(vv);
    expect(project((result as Record<string, ContainerNode>).root)).toEqual(
      m.toJSON(),
    );
  });

  it("bounds recursive traversal and leaves the document usable after an error", () => {
    const d = new LoroDoc();
    let m = d.getMap("deep");
    for (let i = 0; i < 270; i++) m = m.setContainer("child", new LoroMap());
    expect(() => d.toContainerTree()).toThrow("nesting");
    expect(m.toContainerTree()).toEqual({
      type: "Map",
      cid: m.id,
      value: {},
    });
  });

  it("constructs dense arrays without invoking inherited index setters", () => {
    const d = new LoroDoc();
    d.getList("list").push([1, 2]);
    let calls = 0;
    let result: unknown;
    Object.defineProperty(Array.prototype, "0", {
      configurable: true,
      set() {
        calls++;
      },
    });
    try {
      result = d.toContainerTree();
    } finally {
      delete (Array.prototype as unknown as Record<string, unknown>)["0"];
    }
    expect(calls).toBe(0);
    expect(result).toEqual({
      list: {
        type: "List",
        cid: "cid:root-list:List",
        value: [{ type: "Value", value: [1, 2] }],
      },
    });
  });

  it("honors root visibility and returns empty implicit roots", () => {
    const d = new LoroDoc();
    expect(d.getMap("empty").toContainerTree()).toEqual({
      type: "Map",
      cid: "cid:root-empty:Map",
      value: {},
    });
    expect(
      Object.fromEntries(
        Object.entries(d.toContainerTree()).map(([k, v]) => [k, project(v)]),
      ),
    ).toEqual(d.toJSON());
    d.getMap("empty");
    d.setHideEmptyRootContainers(true);
    expect(d.toContainerTree()).toEqual({});
    d.getCounter("counter").increment(3);
    expect(d.toContainerTree().counter).toEqual({
      type: "Counter",
      cid: "cid:root-counter:Counter",
      value: 3,
    });
    d.deleteRootContainer("cid:root-counter:Counter");
    expect(d.toContainerTree()).toEqual({});
  });
});

it("selects roots without traversing excluded subtrees or creating missing roots", () => {
  const d = new LoroDoc();
  d.getMap("selected").set("ok", true);
  let deep = d.getMap("excluded");
  for (let i = 0; i < 270; i++)
    deep = deep.setContainer("child", new LoroMap());
  const vv = d.version().encode();
  const shallow = d.getShallowValue();
  expect(
    d.toContainerTree({ roots: ["selected", "missing", "selected"] }),
  ).toEqual({ selected: d.getMap("selected").toContainerTree() });
  expect(d.toContainerTree({ roots: [] })).toEqual({});
  expect(d.getShallowValue()).toEqual(shallow);
  expect(d.version().encode()).toEqual(vv);
  expect(() => d.toContainerTree()).toThrow("nesting");
});
it("propagates text format through maps, lists and Tree metadata", () => {
  const d = new LoroDoc();
  const map = d.getMap("root");
  const list = map.setContainer("list", new LoroList());
  const text = list
    .pushContainer(new LoroMap())
    .setContainer("text", new LoroText());
  text.insert(0, "hello");
  text.mark({ start: 0, end: 5 }, "bold", true);
  const tree = d.getTree("tree");
  const node = tree.createNode();
  node.data.setContainer("text", new LoroText()).insert(0, "child");
  const roots = d.toContainerTree({ text: "delta" });
  expect(roots.root).toEqual(map.toContainerTree({ text: "delta" }));
  expect(roots.tree).toEqual(tree.toContainerTree({ text: "delta" }));
  const nested = map.toContainerTree({ text: "delta" }).value.list;
  if (nested.type !== "List") throw Error("list");
  const child = nested.value[0];
  if (child.type !== "Map") throw Error("map");
  expect(child.value.text).toEqual({
    type: "Text",
    cid: text.id,
    value: text.toDelta(),
  });
  expect(list.toContainerTreeSlice(0, 1, { text: "delta" }).items).toEqual(
    nested.value,
  );
});
it("rejects detached containers and invalid slice bounds", () => {
  for (const container of [
    new LoroMap(),
    new LoroList(),
    new LoroMovableList(),
    new LoroText(),
    new LoroTree(),
    new LoroCounter(),
  ]) {
    expect(() => container.toContainerTree()).toThrow("attached");
  }
  const d = new LoroDoc();
  const list = d.getList("list");
  list.push("ok");
  for (const n of [-1, 0.5, NaN, Infinity, 2 ** 32]) {
    expect(() => list.toContainerTreeSlice(n, 1)).toThrow("bounds");
    expect(() => list.toContainerTreeSlice(0, n)).toThrow("bounds");
  }
  expect(list.toContainerTreeSlice(0, 99)).toEqual({
    cid: list.id,
    start: 0,
    totalLength: 1,
    items: [{ type: "Value", value: "ok" }],
  });
  expect(d.getCounter("counter").toContainerTree()).toEqual({
    type: "Counter",
    cid: "cid:root-counter:Counter",
    value: 0,
  });
});
// Compile-only API contracts. This function is deliberately never called.
function checkContainerTreeTypes(
  d: LoroDoc,
  map: LoroMap,
  list: LoroList,
  text: LoroText,
) {
  const plain: string = text.toContainerTree().value;
  const delta: ReturnType<LoroText["toDelta"]> = text.toContainerTree({
    text: "delta",
  }).value;
  const mapKind: "Map" = map.toContainerTree().type;
  // @ts-expect-error Document selection is by roots, not a container ID.
  d.toContainerTree({ container: map.id });
  // @ts-expect-error Complete tree reads never accept a range.
  list.toContainerTree({ range: { start: 0, end: 1 } });
  // @ts-expect-error Containers do not select document roots.
  map.toContainerTree({ roots: ["root"] });
  // @ts-expect-error Plain text is not a delta.
  const invalid: typeof delta = plain;
  return [plain, delta, mapKind, invalid];
}
void checkContainerTreeTypes;
