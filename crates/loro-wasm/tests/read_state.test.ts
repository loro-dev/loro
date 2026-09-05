import { describe, expect, it } from "vitest";
import {
  LoroDoc,
  LoroMap,
  LoroText,
  LoroList,
  LoroMovableList,
  type ContainerState,
  type StateNode,
} from "../bundler/index";

function project(node: StateNode): unknown {
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
function ids(node: StateNode): string[] {
  if (node.type === "Value") return [];
  if (node.type === "Map")
    return [node.cid, ...Object.values(node.value).flatMap(ids)];
  if (node.type === "List" || node.type === "MovableList")
    return [node.cid, ...node.value.flatMap(ids)];
  return [node.cid];
}

describe("readState", () => {
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
    const read = d.readState();
    expect(project(read.root)).toStrictEqual({
      ...m.toJSON(),
      ["__proto__"]: { safe: true },
    });
    expect(ids(read.root).sort()).toEqual([m.id, l.id, t.id, merged.id].sort());
    const root = read.root as Extract<ContainerState, { type: "Map" }>;
    expect(root.value.fake).toEqual({ type: "Value", value: fake });
    expect(Object.prototype.hasOwnProperty.call(root.value, "__proto__")).toBe(
      true,
    );
    expect(Object.getPrototypeOf(root.value)).toBe(Object.prototype);
    expect(d.readState({ container: m.id })).toStrictEqual(root);
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
    const plain = d.readState().tree;
    expect(project(plain)).toEqual(d.toJSON().tree);
    const rich = d.readState({ container: tree.id, text: "delta" });
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
      const full = d.readState({ container: l.id });
      if (full.type !== "List" && full.type !== "MovableList")
        throw new Error("expected list");
      expect(
        d.readState({ container: l.id, range: { start: 1, end: 2 } }),
      ).toEqual({ ...full, value: full.value.slice(1, 2) });
      for (const range of [
        { start: 3, end: 99 },
        { start: 2, end: 1 },
        { start: 99, end: 100 },
      ]) {
        expect(d.readState({ container: l.id, range })).toEqual({
          ...full,
          value: [],
        });
      }
    }
  });

  it("rejects invalid inputs and remains usable", () => {
    const d = new LoroDoc();
    d.getMap("root").set("ok", true);
    const read = d.readState.bind(d) as (o: unknown) => unknown;
    for (const options of [
      { container: "bad" },
      { container: "cid:99@88:Map" },
      { text: "html" },
      { range: { start: 0, end: 1 } },
      { container: "cid:root-root:Map", range: { start: 0, end: 1 } },
      { container: "cid:root-l:List", range: { start: -1, end: 1 } },
    ]) {
      expect(() => read(options)).toThrow();
      expect(project(d.readState().root)).toEqual({ ok: true });
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
    let result: ReturnType<LoroDoc["readState"]>;
    try {
      result = d.readState();
    } finally {
      delete (Object.prototype as Record<string, unknown>).__read_state_probe;
    }
    expect(calls).toBe(0);
    expect(d.version().encode()).toEqual(vv);
    expect(project((result as Record<string, ContainerState>).root)).toEqual(
      m.toJSON(),
    );
  });

  it("bounds recursive traversal and leaves the document usable after an error", () => {
    const d = new LoroDoc();
    let m = d.getMap("deep");
    for (let i = 0; i < 270; i++) m = m.setContainer("child", new LoroMap());
    expect(() => d.readState()).toThrow("nesting");
    expect(d.readState({ container: m.id })).toEqual({
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
      result = d.readState();
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
    expect(d.readState({ container: "cid:root-empty:Map" })).toEqual({
      type: "Map",
      cid: "cid:root-empty:Map",
      value: {},
    });
    expect(
      Object.fromEntries(
        Object.entries(d.readState()).map(([k, v]) => [k, project(v)]),
      ),
    ).toEqual(d.toJSON());
    d.getMap("empty");
    d.setHideEmptyRootContainers(true);
    expect(d.readState()).toEqual({});
    d.getCounter("counter").increment(3);
    expect(d.readState().counter).toEqual({
      type: "Counter",
      cid: "cid:root-counter:Counter",
      value: 3,
    });
    d.deleteRootContainer("cid:root-counter:Counter");
    expect(d.readState()).toEqual({});
  });
});
