/* eslint-disable no-console */

// MovableList scaling benchmark. Uses only the public API so that it runs
// against any loro.js build; see context/loro-js-movable-list.md.

import { performance } from "node:perf_hooks";

import { LoroDoc } from "../dist/index.js";

const positionalArguments = process.argv.slice(2).filter((argument) => argument !== "--");
const sizes = (positionalArguments[0] ?? "1000,2000,4000,8000").split(",").map(Number);

function measure(name, size, callback) {
  globalThis.gc?.();
  const start = performance.now();
  const result = callback();
  const milliseconds = performance.now() - start;
  console.log(JSON.stringify({ name, size, milliseconds, result }));
  return result;
}

function random(seed) {
  let state = seed >>> 0;
  return (limit) => {
    state = (state + 0x6d2b79f5) >>> 0;
    let value = state;
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return (((value ^ (value >>> 14)) >>> 0) % limit) | 0;
  };
}

function filled(peer, size) {
  const doc = new LoroDoc();
  doc.setPeerId(peer);
  const list = doc.getMovableList("list");
  for (let index = 0; index < size; index += 1) list.push(index);
  doc.commit();
  return doc;
}

/** `count` random moves (and a set every fourth op), committing every 50 ops. */
function edit(doc, count, seed) {
  const list = doc.getMovableList("list");
  const next = random(seed);
  for (let index = 0; index < count; index += 1) {
    if (index % 4 === 3) list.set(next(list.length), -index);
    else list.move(next(list.length), next(list.length));
    if (index % 50 === 49) doc.commit();
  }
  doc.commit();
}

for (const size of sizes) {
  measure("movable-push", size, () => filled(1, size).getMovableList("list").length);

  const base = filled(1, size);
  const baseVersion = base.oplogVersion();
  const baseFrontiers = base.frontiers();
  const local = base.fork();
  local.setPeerId(2);
  measure("movable-local-move-set", size, () => {
    edit(local, size, 7);
    return local.getMovableList("list").length;
  });
  const update = local.export({ mode: "update", from: baseVersion });

  measure("movable-import-moves", size, () => {
    const target = base.fork();
    target.import(update);
    return target.getMovableList("list").length;
  });

  const concurrent = base.fork();
  concurrent.setPeerId(3);
  edit(concurrent, size, 11);
  const concurrentUpdate = concurrent.export({ mode: "update", from: baseVersion });
  measure("movable-import-concurrent-moves", size, () => {
    const target = local.fork();
    target.import(concurrentUpdate);
    return target.getMovableList("list").length;
  });

  const merged = local.fork();
  merged.import(concurrentUpdate);
  measure(
    "movable-snapshot-export",
    size,
    () => merged.export({ mode: "snapshot" }).length,
  );
  const bytes = merged.export({ mode: "snapshot" });
  measure("movable-snapshot-import-to-json", size, () => {
    const target = new LoroDoc();
    target.import(bytes);
    return target.getMovableList("list").toJSON().length;
  });
  measure("movable-snapshot-import-concurrent-update", size, () => {
    const target = new LoroDoc();
    target.import(local.export({ mode: "snapshot" }));
    target.import(concurrentUpdate);
    return target.getMovableList("list").length;
  });

  const latest = merged.frontiers();
  measure("movable-checkout-before-moves", size, () => {
    merged.checkout(baseFrontiers);
    return merged.getMovableList("list").length;
  });
  measure("movable-checkout-latest", size, () => {
    merged.checkout(latest);
    merged.attach();
    return merged.getMovableList("list").length;
  });
  measure("movable-metadata", size, () => {
    const list = merged.getMovableList("list");
    let checksum = 0;
    for (let index = 0; index < list.length; index += 1) {
      checksum +=
        Number(list.getLastMoverAt(index) ?? 0) +
        Number(list.getLastEditorAt(index) ?? 0);
    }
    return checksum;
  });
}
