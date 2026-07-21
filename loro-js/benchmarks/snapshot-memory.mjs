/* eslint-disable no-console */

import { readFileSync } from "node:fs";
import process from "node:process";

import { LoroDoc } from "../dist/index.js";

const positionalArguments = process.argv.slice(2).filter((argument) => argument !== "--");
const snapshotPath = positionalArguments[0];
const rootName = positionalArguments[1] ?? "root";
if (snapshotPath === undefined) {
  throw new TypeError(
    "usage: pnpm bench:snapshot-memory -- <snapshot-path> [root-map-name]",
  );
}

const mib = 1024 * 1024;
const phases = [];
function measure(phase, extra = {}) {
  globalThis.gc?.();
  const memory = process.memoryUsage();
  const sample = {
    phase,
    heapUsedMiB: memory.heapUsed / mib,
    externalMiB: memory.external / mib,
    rssMiB: memory.rss / mib,
    maxRssMiB: process.resourceUsage().maxRSS / 1024,
    ...extra,
  };
  phases.push(sample);
  console.log(JSON.stringify(sample));
}

function timed(callback) {
  const start = performance.now();
  const value = callback();
  return { value, milliseconds: performance.now() - start };
}

const input = readFileSync(snapshotPath);
measure("input-loaded", { bytes: input.length });
const baselineRss = phases[0].rssMiB;

const doc = new LoroDoc();
let result = timed(() => doc.import(input));
measure("snapshot-imported", { milliseconds: result.milliseconds });

result = timed(() => {
  doc.getMap(rootName).set("__loro_js_memory_bench_local", 1);
  doc.commit();
});
measure("local-change-committed", { milliseconds: result.milliseconds });

const remote = new LoroDoc();
remote.setPeerId(0xffff_ffffn);
remote.getMap(rootName).set("__loro_js_memory_bench_remote", 2);
remote.commit();
const remoteUpdate = remote.export({ mode: "update" });
result = timed(() => doc.import(remoteUpdate));
measure("update-imported", {
  milliseconds: result.milliseconds,
  bytes: remoteUpdate.length,
});

result = timed(() => doc.export({ mode: "update" }));
const update = result.value;
measure("update-exported", {
  milliseconds: result.milliseconds,
  bytes: update.length,
});

result = timed(() => doc.export({ mode: "snapshot" }));
const snapshot = result.value;
measure("snapshot-exported", {
  milliseconds: result.milliseconds,
  bytes: snapshot.length,
});

const peakRss = process.resourceUsage().maxRSS / 1024;
console.log(
  JSON.stringify({
    phase: "summary",
    baselineRssMiB: baselineRss,
    peakRssMiB: peakRss,
    incrementalPeakRssMiB: peakRss - baselineRss,
    peakHeapUsedMiB: Math.max(...phases.map(({ heapUsedMiB }) => heapUsedMiB)),
    under100MiB: peakRss - baselineRss < 100,
  }),
);
