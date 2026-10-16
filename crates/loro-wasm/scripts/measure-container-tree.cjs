// Build the release package first. Run with --count-imports separately from timings.
const assert = require("node:assert/strict");
const { performance } = require("node:perf_hooks");

const countImports = process.argv.includes("--count-imports");
let counting = false;
let decodes = 0;
const calls = new Map();
if (countImports) {
  const decode = TextDecoder.prototype.decode;
  TextDecoder.prototype.decode = function (...args) {
    if (counting) decodes++;
    return decode.apply(this, args);
  };
  const Instance = WebAssembly.Instance;
  WebAssembly.Instance = new Proxy(Instance, {
    construct(target, [module, imports]) {
      const wrapped = Object.fromEntries(
        Object.entries(imports).map(([namespace, values]) => [
          namespace,
          Object.fromEntries(
            Object.entries(values).map(([name, value]) => [
              name,
              typeof value !== "function"
                ? value
                : (...args) => {
                    if (counting) calls.set(name, (calls.get(name) || 0) + 1);
                    return value(...args);
                  },
            ]),
          ),
        ]),
      );
      return Reflect.construct(target, [module, wrapped]);
    },
  });
}

const {
  LoroDoc,
  LoroMap,
  LoroList,
  LoroText,
  isContainer,
} = require("../nodejs/index.js");
const blocks = Number(process.env.BLOCKS || 1108);
const rounds = 11;
const warmups = 3;

function makeSnapshot() {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  const root = doc.getList("blocks");
  let nodes = 0;
  let texts = 0;
  function node(parent, withText) {
    const map = parent.pushContainer(new LoroMap());
    map.set("nodeName", withText ? "paragraph" : "section");
    map.setContainer("attributes", new LoroMap()).set("index", nodes++);
    const children = map.setContainer("children", new LoroList());
    if (withText && texts++ < Math.round((blocks * 1935) / 1108)) {
      const text = children.pushContainer(new LoroText());
      text.insert(
        0,
        "A rich-text note with 中文 and 🙂, links and formatting. ".repeat(2),
      );
      text.mark({ start: 0, end: 12 }, "bold", true);
      text.mark({ start: 20, end: 30 }, "link", "https://example.com");
    }
    return children;
  }
  for (let i = 0; i < blocks; i++) {
    const children = node(root, false);
    for (let j = 0; j < (i < Math.round((blocks * 215) / 1108) ? 3 : 2); j++) {
      node(children, true);
    }
  }
  const snapshot = doc.export({ mode: "snapshot" });
  doc.free();
  return {
    snapshot,
    containers:
      1 + nodes * 3 + Math.min(texts, Math.round((blocks * 1935) / 1108)),
  };
}

function handleNode(value) {
  if (!isContainer(value)) return { type: "Value", value };
  const type = value.kind();
  const cid = value.id;
  if (type === "Text") return { type, cid, value: value.toDelta() };
  if (type === "List")
    return { type, cid, value: value.toArray().map(handleNode) };
  if (type === "Map") {
    return {
      type,
      cid,
      value: Object.fromEntries(
        value.keys().map((key) => [key, handleNode(value.get(key))]),
      ),
    };
  }
  throw new Error(`Unexpected fixture container: ${type}`);
}

const { snapshot, containers } = makeSnapshot();
function fresh() {
  const doc = new LoroDoc();
  doc.import(snapshot);
  return doc;
}
const cases = {
  handles: (doc) => ({ blocks: handleNode(doc.getList("blocks")) }),
  object: (doc) => doc.toContainerTree({ text: "delta" }),
};
const probe = fresh();
if (typeof probe.toContainerTreeJson === "function") {
  const json = probe.toContainerTreeJson({ text: "delta" });
  assert.equal(typeof json, "string");
  cases["json+parse"] = (doc) =>
    JSON.parse(doc.toContainerTreeJson({ text: "delta" }));
}
probe.free();

let expected;
for (const read of Object.values(cases)) {
  const doc = fresh();
  const result = read(doc);
  if (expected) assert.deepStrictEqual(result, expected);
  else expected = result;
  doc.free();
}
expected = undefined;

const result = {};
if (countImports) {
  for (const [name, read] of Object.entries(cases)) {
    const doc = fresh();
    calls.clear();
    decodes = 0;
    counting = true;
    read(doc);
    counting = false;
    result[name] = {
      total: [...calls.values()].reduce((a, b) => a + b, 0),
      stringDecodes: decodes,
      imports: Object.fromEntries(calls),
    };
    doc.free();
  }
} else {
  const samples = Object.fromEntries(
    Object.keys(cases).map((name) => [name, []]),
  );
  for (let round = 0; round < rounds + warmups; round++) {
    const entries = Object.entries(cases);
    if (round % 2) entries.reverse();
    for (const [name, read] of entries) {
      global.gc?.();
      const doc = fresh();
      const start = performance.now();
      const value = read(doc);
      const elapsed = performance.now() - start;
      assert.ok(value.blocks);
      if (round >= warmups) samples[name].push(elapsed);
      doc.free();
    }
  }
  for (const [name, times] of Object.entries(samples)) {
    result[name] = {
      medianMs: [...times].sort((a, b) => a - b)[Math.floor(times.length / 2)],
      samplesMs: times,
    };
  }
}
console.log(
  JSON.stringify(
    {
      node: process.version,
      blocks,
      containers,
      snapshotBytes: snapshot.length,
      rounds,
      warmups,
      countImports,
      result,
    },
    null,
    2,
  ),
);
