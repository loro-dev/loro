const { performance } = require("node:perf_hooks");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const modulePath =
  process.env.LORO_BENCH_MODULE ||
  path.resolve(__dirname, "../nodejs/index.js");
const { LoroDoc, LoroList, LoroMap, LoroText } = require(modulePath);

const MAP_COUNT = 15_632;
const LIST_COUNT = 9_956;
const TEXT_COUNT = 44_463;
const PARENT_POOL_SIZE = 2_048;
const PARAGRAPH = "The quick brown fox jumps over the lazy dog. Pack my box. ";

const WARMUP_ROUNDS = 2;
const ROUNDS = 5;
let blackhole = 0;

function buildDocument() {
  const doc = new LoroDoc();
  doc.setPeerId("1");
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

function decode(result, project = false) {
  let next = 0,
    position = 0;
  const registry = new Set();
  function walk(value) {
    const cid =
      result.containerPositions[next] === position++
        ? result.cids[next++]
        : undefined;
    if (Array.isArray(value)) {
      for (let i = 0; i < value.length; i++) value[i] = walk(value[i]);
    } else if (value !== null && typeof value === "object") {
      for (const key of Object.keys(value)) value[key] = walk(value[key]);
    }
    if (cid === undefined) return value;
    if (!project) return { cid, value };
    registry.add(cid);
    if (cid.endsWith(":Map"))
      Object.defineProperty(value, "$cid", { value: cid });
    return value;
  }
  const value = walk(JSON.parse(result.json));
  assert.equal(next, result.cids.length);
  return project ? { value, registry } : value;
}

function pathsFor(result) {
  let next = 0,
    position = 0;
  const paths = [],
    stack = [];
  function walk(value) {
    if (result.containerPositions[next] === position++) {
      paths.push(stack.slice());
      next++;
    }
    if (value !== null && typeof value === "object") {
      for (const key of Object.keys(value)) {
        stack.push(Array.isArray(value) ? Number(key) : key);
        walk(value[key]);
        stack.pop();
      }
    }
  }
  walk(JSON.parse(result.json));
  assert.equal(next, result.cids.length);
  return paths;
}

function attachPaths(json, cids, paths) {
  let value = JSON.parse(json);
  // Descendants first so replacing a parent does not invalidate its paths.
  for (let i = paths.length - 1; i >= 0; i--) {
    const steps = paths[i];
    if (!steps.length) {
      value = { cid: cids[i], value };
      continue;
    }
    let parent = value;
    for (let j = 0; j < steps.length - 1; j++) parent = parent[steps[j]];
    const key = steps[steps.length - 1];
    parent[key] = { cid: cids[i], value: parent[key] };
  }
  return value;
}

function handleProjection(container, registry) {
  const kind = container.kind();
  const cid = container.id;
  registry.add(cid);
  let out;
  if (kind === "Map") {
    out = {};
    Object.defineProperty(out, "$cid", { value: cid });
    for (const key of container.keys()) {
      const child = container.get(key);
      out[key] =
        child && typeof child.kind === "function"
          ? handleProjection(child, registry)
          : child;
    }
  } else if (kind === "List" || kind === "MovableList") {
    out = [];
    for (let i = 0, n = container.length; i < n; i++) {
      const child = container.get(i);
      out.push(
        child && typeof child.kind === "function"
          ? handleProjection(child, registry)
          : child,
      );
    }
  } else {
    out = container.toJSON();
  }
  container.free();
  return out;
}

const name = process.argv[2];
if (name) {
  // Each case gets its own process and a freshly imported snapshot. Do not
  // hide allocations in an already materialized, directly built document.
  const snapshot = fs.readFileSync(process.argv[3]);
  const doc = new LoroDoc();
  const startImport = performance.now();
  doc.import(snapshot);
  const importMs = performance.now() - startImport;
  const run = {
    toJSON: () => doc.toJSON(),
    getDeepValueWithID: () => doc.getDeepValueWithID(),
    jsonParse: () => JSON.parse(doc.getDeepValueJson()),
    indexedJson: () => doc.getDeepValueJsonWithIds(),
    legacyJson: () => doc.getDeepValueJsonWithIds(),
    indexedJsonParseReattach: () => decode(doc.getDeepValueJsonWithIds()),
    indexedProjection: () => decode(doc.getDeepValueJsonWithIds(), true),
    handleProjection: () => {
      const registry = new Set();
      return {
        value: { root: handleProjection(doc.getMap("root"), registry) },
        registry,
      };
    },
  }[name];
  global.gc?.();
  const before = process.memoryUsage();
  const start = performance.now();
  let held = run();
  const coldMs = performance.now() - start;
  global.gc?.();
  const after = process.memoryUsage();
  const memory = Object.fromEntries(
    ["external", "heapUsed", "rss"].map((k) => [
      k + "DeltaBytes",
      after[k] - before[k],
    ]),
  );
  // Correctness outside timed regions; a faster read of the wrong shape is
  // not an optimization. Projection includes cid registration and map $cid.
  if (name === "indexedJson" || name === "indexedJsonParseReattach") {
    assert.deepStrictEqual(
      name === "indexedJson" ? decode(held) : held,
      doc.getDeepValueWithID(),
    );
  } else if (name === "indexedProjection" || name === "handleProjection") {
    assert.deepStrictEqual(held.value, doc.toJSON());
    assert.equal(held.registry.size, MAP_COUNT + LIST_COUNT + TEXT_COUNT);
    assert.equal(held.value.root.$cid, doc.getMap("root").id);
  } else if (name === "legacyJson") {
    assert.deepStrictEqual(
      JSON.parse(held.json),
      JSON.parse(doc.getDeepValueJson()),
    );
  } else if (name !== "getDeepValueWithID") {
    assert.deepStrictEqual(held, doc.toJSON());
  }
  held = null;
  const samples = [];
  for (let round = 0; round < WARMUP_ROUNDS + ROUNDS; round++) {
    global.gc?.();
    const t = performance.now();
    held = run();
    const elapsed = performance.now() - t;
    blackhole += Object.keys(held).length;
    held = null;
    if (round >= WARMUP_ROUNDS) samples.push(elapsed);
  }
  samples.sort((a, b) => a - b);
  console.log(
    JSON.stringify({
      name,
      importMs,
      coldMs,
      warmMedianMs: samples[Math.floor(samples.length / 2)],
      ...memory,
      blackhole,
    }),
  );
  doc.free();
} else {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "loro-bulk-bench-"));
  try {
    const doc = buildDocument();
    const file = path.join(dir, "fixture.bin");
    fs.writeFileSync(file, doc.export({ mode: "snapshot" }));
    const result = doc.getDeepValueJsonWithIds();
    const supportsPositions = result.containerPositions instanceof Uint32Array;
    const cases = [
      "toJSON",
      "getDeepValueWithID",
      "jsonParse",
      "handleProjection",
    ];
    let metadata;
    if (supportsPositions) {
      cases.push(
        "indexedJson",
        "indexedJsonParseReattach",
        "indexedProjection",
      );
      const paths = pathsFor(result);
      assert.deepStrictEqual(
        attachPaths(result.json, result.cids, paths),
        decode(result),
      );
      const positionSamples = [],
        pathSamples = [];
      for (let i = 0; i < WARMUP_ROUNDS + ROUNDS; i++) {
        global.gc?.();
        let t = performance.now();
        decode(result);
        const posMs = performance.now() - t;
        t = performance.now();
        attachPaths(result.json, result.cids, paths);
        const pathMs = performance.now() - t;
        if (i >= WARMUP_ROUNDS) {
          positionSamples.push(posMs);
          pathSamples.push(pathMs);
        }
      }
      const median = (xs) =>
        xs.sort((a, b) => a - b)[Math.floor(xs.length / 2)];
      metadata = {
        positionsBytes: result.containerPositions.byteLength,
        pathsJsonBytes: Buffer.byteLength(JSON.stringify(paths)),
        pathSegments: paths.reduce((n, p) => n + p.length, 0),
        cidsJsonBytes: Buffer.byteLength(JSON.stringify(result.cids)),
        // These isolate JS consumption; path generation/transfer is excluded.
        positionsParseAttachMs: median(positionSamples),
        pathsParseAttachMs: median(pathSamples),
      };
    }
    if (!supportsPositions) cases.push("legacyJson");
    doc.free();
    const measurements = cases.map((name) => {
      const child = spawnSync(
        process.execPath,
        ["--expose-gc", __filename, name, file],
        { encoding: "utf8", env: process.env },
      );
      if (child.status !== 0) throw new Error(child.stderr || child.stdout);
      return JSON.parse(child.stdout);
    });
    console.log(
      JSON.stringify(
        {
          containers: MAP_COUNT + LIST_COUNT + TEXT_COUNT,
          jsonBytes: Buffer.byteLength(result.json),
          metadata,
          measurements,
        },
        null,
        2,
      ),
    );
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}
