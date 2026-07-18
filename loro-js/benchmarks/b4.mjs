/* eslint-disable no-console */

import { readFileSync } from "node:fs";
import { performance } from "node:perf_hooks";
import { gunzipSync } from "node:zlib";

import { LoroDoc } from "../dist/index.js";

const tracePath = new URL(
  "../../crates/loro-internal/benches/automerge-paper.json.gz",
  import.meta.url,
);
const actions = JSON.parse(gunzipSync(readFileSync(tracePath))).txns.flatMap(
  (transaction) => transaction.patches,
);
const positionalArguments = process.argv.slice(2).filter((argument) => argument !== "--");
const sizes = (positionalArguments[0] ?? String(actions.length)).split(",").map(Number);
const runs = Number(positionalArguments[1] ?? 1);

function apply(count) {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const text = doc.getText("text");
  for (let index = 0; index < count; index += 1) {
    const [position, deleted, inserted] = actions[index];
    text.delete(position, deleted);
    text.insert(position, inserted);
  }
  doc.commit();
  return doc;
}

function median(values) {
  const sorted = values.toSorted((left, right) => left - right);
  return sorted[Math.floor(sorted.length / 2)];
}

function measure(count, callback) {
  const samples = [];
  let value;
  for (let run = 0; run < count; run += 1) {
    value = undefined;
    globalThis.gc?.();
    const start = performance.now();
    value = callback();
    samples.push(performance.now() - start);
  }
  return { milliseconds: median(samples), samples, value };
}

for (const size of sizes) {
  if (!Number.isSafeInteger(size) || size < 0 || size > actions.length) {
    throw new RangeError(`B4 prefix ${size} is outside 0..${actions.length}`);
  }
}
apply(Math.max(...sizes));
let fullDocument;
for (const size of sizes) {
  const result = measure(runs, () => apply(size));
  const doc = result.value;
  if (size === actions.length) fullDocument = doc;
  globalThis.gc?.();
  const memory = process.memoryUsage();
  const text = doc.getText("text");
  console.log(
    JSON.stringify({
      phase: "apply",
      actions: size,
      milliseconds: result.milliseconds,
      samples: result.samples,
      finalLength: text.length,
      sequenceElements: text._sequence.allLength,
      sequenceSpans: text._sequence.spanCount,
      heapUsedBytes: memory.heapUsed,
      rssBytes: memory.rss,
    }),
  );
}

if (fullDocument !== undefined && positionalArguments[2] !== "apply-only") {
  const snapshotExport = measure(3, () => fullDocument.export({ mode: "snapshot" }));
  const snapshot = snapshotExport.value;
  console.log(
    JSON.stringify({
      phase: "snapshot-export",
      milliseconds: snapshotExport.milliseconds,
      samples: snapshotExport.samples,
      bytes: snapshot.length,
    }),
  );
  const updateExport = measure(3, () => fullDocument.export({ mode: "update" }));
  const update = updateExport.value;
  console.log(
    JSON.stringify({
      phase: "update-export",
      milliseconds: updateExport.milliseconds,
      samples: updateExport.samples,
      bytes: update.length,
    }),
  );
  for (const [phase, bytes] of [
    ["snapshot-import", snapshot],
    ["update-import", update],
  ]) {
    const imported = measure(3, () => {
      const doc = new LoroDoc();
      doc.import(bytes);
      return doc;
    });
    console.log(
      JSON.stringify({
        phase,
        milliseconds: imported.milliseconds,
        samples: imported.samples,
        finalLength: imported.value.getText("text").length,
      }),
    );
  }
}
