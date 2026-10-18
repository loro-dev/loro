import { expect, it } from "vitest";
import {
  LoroDoc,
  LoroList,
  LoroMap,
  LoroMovableList,
  LoroText,
  type DocumentContainerTreeOptions,
} from "../bundler/index";

it("serializes the existing node shapes, IDs, opaque values and text formats", () => {
  const source = new LoroDoc();
  source.setPeerId("18446744073709551614");
  const map = source.getMap("__proto__");
  map.set("__proto__", { ["__proto__"]: { safe: true } });
  map.set('quote"\\\n', [null, true, false, '\uFEFF中🙂\u0000\n\t"\\']);
  map.set("fake", {
    type: "Map",
    cid: "cid:root-fake:Map",
    value: { text: "opaque" },
  });
  map.setContainer("empty", new LoroMap());
  const list = map.setContainer("list", new LoroList());
  list.pushContainer(new LoroMovableList()).push("item");
  map.ensureMergeableMap("merged").set("ok", true);
  const text = list.pushContainer(new LoroText());
  text.insert(0, "hello中🙂 world");
  text.mark({ start: 0, end: 2 }, "bold", true);
  text.mark({ start: 2, end: 5 }, "bold", true);
  text.mark({ start: 1, end: 4 }, "italic", true);
  text.unmark({ start: 1, end: 4 }, "italic");
  // Tree reads already merge equal attributes after null marks are dropped.
  // toDelta() can retain those span boundaries; do not change either existing API.
  const delta = [
    { insert: "hello", attributes: { bold: true } },
    { insert: "中🙂 world" },
  ];
  const tree = source.getTree("tree");
  const node = tree.createNode();
  node.data.setContainer("text", new LoroText()).insert(0, "metadata");
  node.createNode().data.set("title", "child");
  source.getCounter("counter").increment(1.25);
  const snapshot = source.export({ mode: "snapshot" });
  const imported = new LoroDoc();
  imported.import(snapshot);

  for (const doc of [source, imported]) {
    for (const text of ["plain", "delta"] as const) {
      const json = doc.toContainerTreeJson({ text });
      expect(typeof json).toBe("string");
      const parsed = JSON.parse(json);
      expect(parsed).toStrictEqual(doc.toContainerTree({ text }));
      expect(Object.prototype.hasOwnProperty.call(parsed, "__proto__")).toBe(
        true,
      );
      expect(Object.getPrototypeOf(parsed)).toBe(Object.prototype);
      expect(
        Object.prototype.hasOwnProperty.call(
          parsed.__proto__.value,
          "__proto__",
        ),
      ).toBe(true);
      if (text === "delta") {
        expect(parsed.__proto__.value.list.value[1].value).toEqual(delta);
      }
    }
    expect(JSON.parse(doc.toContainerTreeJson())).toStrictEqual(
      doc.toContainerTree(),
    );
  }
  const json = imported.toContainerTreeJson({ text: "delta" });
  const expected = imported.toContainerTree({ text: "delta" });
  imported.free();
  expect(JSON.parse(json)).toStrictEqual(expected);
  source.free();
});

it("round trips finite JS numbers, including negative zero and subnormal values", () => {
  const doc = new LoroDoc();
  const numbers = [
    -0,
    Number.MIN_VALUE,
    Number.MAX_VALUE,
    -Number.MAX_VALUE,
    Number.EPSILON,
    1.0000000000000002,
    1000000000000000100,
    9007199254740992,
    -9007199254740992,
  ];
  const list = doc.getList("numbers");
  for (const n of numbers) list.push(n);
  expect(JSON.parse(doc.toContainerTreeJson())).toStrictEqual(
    doc.toContainerTree(),
  );
  doc.free();
});

it("selects visible roots before traversal and never commits pending changes", () => {
  const doc = new LoroDoc();
  doc.setHideEmptyRootContainers(true);
  doc.getMap("empty");
  doc.getMap("selected").set("pending", true);
  doc.getMap("binary").set("bytes", new Uint8Array([1, 2]));
  let deep = doc.getMap("deep");
  for (let i = 0; i < 270; i++)
    deep = deep.setContainer("child", new LoroMap());
  const version = doc.version().encode();
  const shallow = doc.getShallowValue();
  const options: DocumentContainerTreeOptions = {
    roots: ["selected", "missing", "empty", "selected"],
    text: "delta",
  };
  expect(JSON.parse(doc.toContainerTreeJson(options))).toStrictEqual(
    doc.toContainerTree(options),
  );
  expect(doc.toContainerTreeJson({ roots: [] })).toBe("{}");
  expect(() => doc.toContainerTreeJson({ roots: ["deep"] })).toThrow("nesting");
  expect(doc.version().encode()).toEqual(version);
  expect(doc.getShallowValue()).toEqual(shallow);
  doc.free();
});

it("rejects values JSON cannot preserve and keeps the existing read usable", () => {
  for (const value of [new Uint8Array([0, 255]), NaN, Infinity, -Infinity]) {
    const doc = new LoroDoc();
    doc.getMap("root").set("nested", { array: [value] });
    const expected = doc.toContainerTree();
    expect(() => doc.toContainerTreeJson()).toThrow(/binary|non-finite/i);
    expect(doc.toContainerTree()).toStrictEqual(expected);
    doc.getMap("root").set("nested", null);
    expect(JSON.parse(doc.toContainerTreeJson())).toStrictEqual(
      doc.toContainerTree(),
    );
    doc.free();
  }
  const doc = new LoroDoc();
  const text = doc.getText("text");
  text.insert(0, "marked");
  text.mark({ start: 0, end: 6 }, "bold", new Uint8Array([1]));
  expect(() => doc.toContainerTreeJson({ text: "delta" })).toThrow(/binary/i);
  expect(JSON.parse(doc.toContainerTreeJson())).toStrictEqual(
    doc.toContainerTree(),
  );
  doc.free();
});

it("validates options and does not change the existing default API", () => {
  const doc = new LoroDoc();
  doc.getText("text").insert(0, "hello");
  const read = doc.toContainerTreeJson.bind(doc) as (o: unknown) => string;
  for (const options of [
    [],
    1,
    { text: "html" },
    { roots: "text" },
    { format: "json" },
  ]) {
    expect(() => read(options)).toThrow();
  }
  expect(JSON.parse(doc.toContainerTreeJson(undefined))).toStrictEqual(
    doc.toContainerTree(),
  );
  expect(typeof doc.toContainerTree()).toBe("object");
  expect(() =>
    (doc.toContainerTree as (o: unknown) => unknown)({ format: "json" }),
  ).toThrow();
  doc.free();
});

// Compile-only: a new method must not widen the existing object return types.
function checkJsonTypes(doc: LoroDoc, options?: DocumentContainerTreeOptions) {
  const json: string = doc.toContainerTreeJson(options);
  const plain = doc.toContainerTree(options).text;
  const selected = doc.toContainerTree({ roots: ["text"] as const });
  // @ts-expect-error JSON reads return text, not a node tree.
  const object: Record<string, unknown> = doc.toContainerTreeJson();
  // @ts-expect-error Explicit return types cannot override missing options.
  doc.toContainerTreeJson<[{ text: "delta" }]>();
  return [json, plain, selected, object];
}
void checkJsonTypes;
