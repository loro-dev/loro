/* eslint-disable no-console */

import { performance } from "node:perf_hooks";

import { LoroDoc, LoroText } from "../dist/index.js";

const positionalArguments = process.argv.slice(2).filter((argument) => argument !== "--");
const scalarCount = Number(positionalArguments[0] ?? 131_072);
const scalarEdits = Number(positionalArguments[1] ?? 50_000);
const runs = Number(positionalArguments[2] ?? 7);

if (!Number.isSafeInteger(scalarCount) || scalarCount <= 0) {
  throw new RangeError("scalar count must be a positive safe integer");
}
if (!Number.isSafeInteger(scalarEdits) || scalarEdits <= 0) {
  throw new RangeError("scalar edit count must be a positive safe integer");
}
if (!Number.isSafeInteger(runs) || runs <= 0) {
  throw new RangeError("run count must be a positive safe integer");
}

const scalarPattern = Array.from("abc\ud83d\ude00\n");
const payload = Array.from(
  { length: scalarCount },
  (_, index) => scalarPattern[index % scalarPattern.length],
).join("");

function median(values) {
  const sorted = values.toSorted((left, right) => left - right);
  return sorted[Math.floor(sorted.length / 2)];
}

function measure(name, callback, sampleCount = runs, warmupCount = 1) {
  for (let warmup = 0; warmup < warmupCount; warmup += 1) callback();
  const samples = [];
  let result;
  for (let run = 0; run < sampleCount; run += 1) {
    result = undefined;
    globalThis.gc?.();
    const start = performance.now();
    result = callback();
    samples.push(performance.now() - start);
  }
  console.log(
    JSON.stringify({
      name,
      scalarCount,
      scalarEdits,
      milliseconds: median(samples),
      samples,
      result,
    }),
  );
}

measure("scalar-tail-insert", () => {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const text = doc.getText("text");
  for (let index = 0; index < scalarEdits; index += 1) text.push("x");
  doc.commit();
  return {
    length: text.length,
    spans: text._sequence.spanCount,
  };
});

measure("bulk-insert", () => {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const text = doc.getText("text");
  text.insert(0, payload);
  doc.commit();
  return {
    unicodeLength: text._sequence.visibleLength,
    utf16Length: text.length,
    spans: text._sequence.spanCount,
  };
});

const readText = new LoroText();
readText.insert(0, payload);
measure("text-to-string", () => readText.toString().length, runs, 8);
measure(
  "text-slice-middle",
  () => {
    const edge = readText.length >>> 2;
    return readText.slice(edge, readText.length - edge).length;
  },
  runs,
  8,
);
measure(
  "text-iter",
  () => {
    let length = 0;
    readText.iter((chunk) => {
      length += chunk.length;
    });
    return length;
  },
  runs,
  8,
);

const lineCount = payload.split("\n").length;
const targetLine = lineCount >>> 1;
measure(
  "flat-string-line-start-1000",
  () => {
    let checksum = 0;
    for (let iteration = 0; iteration < 1_000; iteration += 1) {
      let offset = 0;
      for (let line = 0; line < targetLine; line += 1) {
        offset = payload.indexOf("\n", offset) + 1;
      }
      checksum += offset;
    }
    return checksum;
  },
  Math.min(runs, 3),
);

if (typeof readText.lineStart === "function") {
  globalThis.gc?.();
  const beforeLineIndex = process.memoryUsage();
  measure("line-index-build", () => readText.lineCount, 1, 0);
  globalThis.gc?.();
  const afterLineIndex = process.memoryUsage();
  console.log(
    JSON.stringify({
      name: "line-index-memory",
      scalarCount,
      scalarEdits,
      heapUsedBytes: afterLineIndex.heapUsed - beforeLineIndex.heapUsed,
      rssBytes: afterLineIndex.rss - beforeLineIndex.rss,
    }),
  );
  measure("indexed-line-start-1000", () => {
    let checksum = 0;
    for (let iteration = 0; iteration < 1_000; iteration += 1) {
      checksum += readText.lineStart(targetLine) ?? 0;
    }
    return checksum;
  });
  measure("indexed-line-at-1000", () => {
    let checksum = 0;
    const position = readText.lineStart(targetLine) ?? 0;
    for (let iteration = 0; iteration < 1_000; iteration += 1) {
      checksum += readText.lineAt(position) ?? 0;
    }
    return checksum;
  });
}

const compactDoc = new LoroDoc();
compactDoc.setPeerId(2);
const compactText = compactDoc.getText("text");
for (let index = 0; index < scalarEdits; index += 1) {
  compactText.insert(compactText.length >>> 1, "x");
}
compactDoc.commit();
const beforeCompactSpans = compactText._sequence.spanCount;
if (typeof compactText.compact === "function") {
  measure("text-compact", () => compactText.compact(), 1, 0);
  console.log(
    JSON.stringify({
      name: "text-compact-spans",
      scalarCount,
      scalarEdits,
      before: beforeCompactSpans,
      after: compactText._sequence.spanCount,
    }),
  );
}

globalThis.gc?.();
const before = process.memoryUsage();
const retainedDoc = new LoroDoc();
retainedDoc.setPeerId(1);
const retainedText = retainedDoc.getText("text");
retainedText.insert(0, payload);
retainedDoc.commit();
globalThis.gc?.();
const after = process.memoryUsage();
console.log(
  JSON.stringify({
    name: "retained-memory",
    scalarCount,
    scalarEdits,
    heapUsedBytes: after.heapUsed - before.heapUsed,
    rssBytes: after.rss - before.rss,
    spans: retainedText._sequence.spanCount,
  }),
);
