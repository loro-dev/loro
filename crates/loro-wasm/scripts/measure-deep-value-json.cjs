const { performance } = require("node:perf_hooks");
const { LoroDoc, LoroList, LoroMap, LoroText } = require("../nodejs/index.js");

// Mirrors the distribution of a real-world document:
// Map 15,632 / List 9,956 / Text 44,463 containers (~70k in total),
// ~3.9 MB of JSON content.
const MAP_COUNT = 15_632;
const LIST_COUNT = 9_956;
const TEXT_COUNT = 44_463;
const PARENT_POOL_SIZE = 2_048;
const PARAGRAPH =
  "The quick brown fox jumps over the lazy dog. Pack my box. ";

const WARMUP_ROUNDS = 2;
const ROUNDS = 5;
let blackhole = 0;

function buildDocument() {
  const doc = new LoroDoc();
  const root = doc.getMap("root");
  const parents = [root];
  let mi = 1;
  let li = 0;
  let ti = 0;
  let seq = 0;
  const addChild = (child, isParent) => {
    const parent = parents[seq % parents.length];
    seq++;
    let attached;
    if (parent.kind() === "Map") {
      attached = parent.setContainer(`k${seq}`, child);
    } else {
      attached = parent.pushContainer(child);
    }
    if (isParent && parents.length < PARENT_POOL_SIZE) parents.push(attached);
    return attached;
  };
  while (mi < MAP_COUNT || li < LIST_COUNT || ti < TEXT_COUNT) {
    if (mi < MAP_COUNT) {
      const map = addChild(new LoroMap(), true);
      map.set("id", mi);
      map.set("flag", true);
      map.set("name", `record-${mi}`);
      mi++;
    }
    if (li < LIST_COUNT) {
      const list = addChild(new LoroList(), true);
      list.insert(0, li);
      list.insert(1, `item-${li}`);
      li++;
    }
    if (ti < TEXT_COUNT) {
      const text = addChild(new LoroText(), false);
      text.insert(0, `${PARAGRAPH}${ti}`);
      ti++;
    }
  }
  doc.commit();
  return doc;
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

function timed(name, run) {
  const samples = [];
  for (let round = 0; round < WARMUP_ROUNDS + ROUNDS; round++) {
    global.gc?.();
    const start = performance.now();
    run();
    const elapsed = performance.now() - start;
    if (round >= WARMUP_ROUNDS) samples.push(elapsed);
  }
  return { name, medianMs: median(samples), samplesMs: samples };
}

// The same pre-order re-attach walk documented for consumers of
// getDeepValueJsonWithIds(): root object -> every value is a container;
// Map -> object entries; List/MovableList/Tree -> arrays; Text -> string;
// Counter -> number.
function makeReattach(cids) {
  let i = 0;
  const containerTypeOf = (cid) => cid.slice(cid.lastIndexOf(":") + 1);
  const shapeMatches = (type, value) => {
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
  const attach = (value) => {
    const cid = cids[i];
    if (cid === undefined || !shapeMatches(containerTypeOf(cid), value)) {
      return value;
    }
    return attachForced(value);
  };
  const attachForced = (value) => {
    const cid = cids[i++];
    return { cid, value: walkContainerValue(containerTypeOf(cid), value) };
  };
  const walkContainerValue = (type, value) => {
    switch (type) {
      case "Text":
      case "Counter":
        return value;
      case "Map": {
        const out = {};
        for (const k of Object.keys(value)) out[k] = attach(value[k]);
        return out;
      }
      case "List":
      case "MovableList":
        return value.map(attach);
      case "Tree":
        return value;
    }
  };
  return (json) => {
    const out = {};
    for (const k of Object.keys(json)) out[k] = attachForced(json[k]);
    blackhole += i;
    return out;
  };
}

const doc = buildDocument();
const stats = {
  containers: MAP_COUNT + LIST_COUNT + TEXT_COUNT,
  jsonBytes: Buffer.byteLength(doc.getDeepValueJson(), "utf8"),
};

const cases = [
  ["toJSON", () => blackhole += JSON.stringify(doc.toJSON()).length],
  ["getDeepValueWithID", () => blackhole += Object.keys(doc.getDeepValueWithID()).length],
  ["getDeepValueJson", () => blackhole += doc.getDeepValueJson().length],
  [
    "getDeepValueJson+parse",
    () => blackhole += Object.keys(JSON.parse(doc.getDeepValueJson())).length,
  ],
  [
    "getDeepValueJsonWithIds+parse+reattach",
    () => {
      const { json, cids } = doc.getDeepValueJsonWithIds();
      const out = makeReattach(cids)(JSON.parse(json));
      blackhole += Object.keys(out).length;
    },
  ],
];

const result = cases.map(([name, run]) => timed(name, run));
const withId = result.find((r) => r.name === "getDeepValueWithID").medianMs;
const jsonParse = result.find(
  (r) => r.name === "getDeepValueJson+parse",
).medianMs;

console.log(
  JSON.stringify(
    {
      ...stats,
      warmupRounds: WARMUP_ROUNDS,
      rounds: ROUNDS,
      blackhole,
      result,
      speedup: {
        "getDeepValueJson+parse vs getDeepValueWithID": withId / jsonParse,
      },
    },
    null,
    2,
  ),
);
doc.free();
