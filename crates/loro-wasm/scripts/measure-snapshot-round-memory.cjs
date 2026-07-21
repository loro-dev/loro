#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { isDeepStrictEqual } = require("node:util");

const MiB = 1024 * 1024;
const packageDir = path.resolve(__dirname, "../nodejs");
const defaultLocalUpdateRounds = 32;
const defaultLocalPayloadBytes = 4 * 1024;

function die(message) {
  console.error(message);
  process.exit(1);
}

function loadLoro() {
  const packageEntry = path.join(packageDir, "index.js");
  if (!fs.existsSync(packageEntry)) {
    die(
      "nodejs package is missing; run `pnpm -C crates/loro-wasm build-release` first",
    );
  }
  return {
    loro: require(packageEntry),
    lowLevel: require(path.join(packageDir, "loro_wasm.js")),
  };
}

function retainedBinaryBytes(root) {
  const seenObjects = new WeakSet();
  const seenBuffers = new Set();
  let bytes = 0;

  function countBuffer(buffer) {
    if (!seenBuffers.has(buffer)) {
      seenBuffers.add(buffer);
      bytes += buffer.byteLength;
    }
  }

  function visit(value) {
    if (value === null || typeof value !== "object") return;
    if (ArrayBuffer.isView(value)) {
      countBuffer(value.buffer);
      return;
    }
    if (
      value instanceof ArrayBuffer ||
      (typeof SharedArrayBuffer !== "undefined" &&
        value instanceof SharedArrayBuffer)
    ) {
      countBuffer(value);
      return;
    }
    if (seenObjects.has(value)) return;
    seenObjects.add(value);
    if (Array.isArray(value)) {
      for (const item of value) visit(item);
      return;
    }
    for (const item of Object.values(value)) visit(item);
  }

  visit(root);
  return bytes;
}

function sampleMemory(lowLevel) {
  const usage = process.memoryUsage();
  return {
    wasmMiB: lowLevel.__wasm.memory.buffer.byteLength / MiB,
    rssMiB: usage.rss / MiB,
    maxRssMiB: process.resourceUsage().maxRSS / 1024,
    heapUsedMiB: usage.heapUsed / MiB,
    externalMiB: usage.external / MiB,
    arrayBuffersMiB: usage.arrayBuffers / MiB,
  };
}

function recordPhase(phases, name, lowLevel) {
  phases[name] = sampleMemory(lowLevel);
}

function makeLocalPayload(size, round) {
  const chars = new Array(size);
  let state = (round + 1) * 0x9e3779b1;
  for (let i = 0; i < size; i += 1) {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    chars[i] = String.fromCharCode(32 + ((state >>> 0) % 95));
  }
  return chars.join("");
}

function applyLocalUpdates(doc, scenario, rounds, payloadBytes) {
  if (rounds === 0) {
    return {
      commitCount: 0,
      mutationCount: 0,
      insertedTextBytes: 0,
    };
  }

  const rootName = `__snapshot_memory_bench_${scenario}`;
  const text = doc.getText(`${rootName}_text`);
  const map = doc.getMap(`${rootName}_map`);

  for (let round = 0; round < rounds; round += 1) {
    const payload = makeLocalPayload(payloadBytes, round);
    text.insert(text.length, payload);
    map.set(`round-${round}`, {
      round,
      payloadBytes,
      marker: payload.slice(0, 16),
    });
    doc.commit();
  }

  return {
    commitCount: rounds,
    mutationCount: rounds * 2,
    insertedTextBytes: rounds * payloadBytes,
  };
}

function runWorker() {
  const snapshotPath = process.argv[3];
  const scenario = process.argv[4];
  const update = Buffer.from(process.argv[5], "base64");
  const budgetMiB = Number(process.argv[6]);
  const localUpdateRounds = Number(process.argv[7]);
  const localPayloadBytes = Number(process.argv[8]);
  const localPeerId = Number(process.argv[9]);
  const { loro, lowLevel } = loadLoro();
  const snapshot = fs.readFileSync(snapshotPath);
  const doc = new loro.LoroDoc();
  global.gc?.();
  const baselineHeapBytes = process.memoryUsage().heapUsed;
  const phases = {};
  recordPhase(phases, "module", lowLevel);

  doc.import(snapshot);
  recordPhase(phases, "afterSnapshotImport", lowLevel);
  const retainedJson = doc.toJSON();
  recordPhase(phases, "afterToJson", lowLevel);
  doc.import(update);
  recordPhase(phases, "afterRemoteUpdateImport", lowLevel);

  const localUpdateStartVersion = doc.oplogVersion();
  doc.setPeerId(localPeerId);
  recordPhase(phases, "afterLocalVersionCapture", lowLevel);
  const localWorkload = applyLocalUpdates(
    doc,
    scenario,
    localUpdateRounds,
    localPayloadBytes,
  );
  recordPhase(phases, "afterLocalUpdates", lowLevel);

  const exportedLocalUpdate = doc.export({
    mode: "update",
    from: localUpdateStartVersion,
  });
  recordPhase(phases, "afterLocalUpdateExport", lowLevel);
  localUpdateStartVersion.free();

  const exportedSnapshot = doc.export({ mode: "snapshot" });
  recordPhase(phases, "afterSnapshotExport", lowLevel);

  // This is the memory directly attributable to the JS/WASM round: input buffers, the Wasm
  // linear-memory high-water mark, both returned export buffers, and the retained JS heap growth
  // from toJSON. RSS is reported separately because it also includes V8 and code pages.
  const wasmBytes = lowLevel.__wasm.memory.buffer.byteLength;
  const exportedLocalUpdateBytes = exportedLocalUpdate.byteLength;
  const exportedSnapshotBytes = exportedSnapshot.byteLength;
  global.gc?.();
  const usageAfterExport = process.memoryUsage();
  const processMaxRssMiB = process.resourceUsage().maxRSS / 1024;
  const retainedHeapBytes = Math.max(
    usageAfterExport.heapUsed - baselineHeapBytes,
    0,
  );
  const retainedJsonExternalBytes = retainedBinaryBytes(retainedJson);
  const controlledBytes =
    snapshot.byteLength +
    update.byteLength +
    wasmBytes +
    exportedLocalUpdateBytes +
    exportedSnapshotBytes +
    retainedHeapBytes +
    retainedJsonExternalBytes;
  const retainedJsonRootCount = Object.keys(retainedJson).length;

  const expectedVersion = doc.version();
  const expectedVersionBytes = Buffer.from(expectedVersion.encode());
  const expectedJson = doc.toJSON();
  const updateRoundTripped = new loro.LoroDoc();
  updateRoundTripped.import(snapshot);
  updateRoundTripped.import(update);
  updateRoundTripped.import(exportedLocalUpdate);
  const updateRoundTripVersion = updateRoundTripped.version();
  const updateRoundTripVersionMatches =
    expectedVersion.compare(updateRoundTripVersion) === 0;
  const updateRoundTripVersionEncodingMatches = Buffer.from(
    updateRoundTripVersion.encode(),
  ).equals(expectedVersionBytes);
  const updateRoundTripJsonMatches = isDeepStrictEqual(
    updateRoundTripped.toJSON(),
    expectedJson,
  );
  const snapshotRoundTripped = new loro.LoroDoc();
  snapshotRoundTripped.import(exportedSnapshot);
  const snapshotRoundTripVersion = snapshotRoundTripped.version();
  const snapshotRoundTripVersionMatches =
    expectedVersion.compare(snapshotRoundTripVersion) === 0;
  const snapshotRoundTripVersionEncodingMatches = Buffer.from(
    snapshotRoundTripVersion.encode(),
  ).equals(expectedVersionBytes);
  const snapshotRoundTripJsonMatches = isDeepStrictEqual(
    snapshotRoundTripped.toJSON(),
    expectedJson,
  );
  expectedVersion.free();
  updateRoundTripVersion.free();
  snapshotRoundTripVersion.free();
  const underBudget = controlledBytes < budgetMiB * MiB;
  const phaseValues = Object.values(phases);
  const localUpdatesWasmGrowthMiB =
    phases.afterLocalUpdates.wasmMiB - phases.afterRemoteUpdateImport.wasmMiB;
  const localUpdateExportWasmGrowthMiB =
    phases.afterLocalUpdateExport.wasmMiB - phases.afterLocalUpdates.wasmMiB;
  const snapshotExportWasmGrowthMiB =
    phases.afterSnapshotExport.wasmMiB - phases.afterLocalUpdateExport.wasmMiB;

  const result = {
    scenario,
    inputSnapshotBytes: snapshot.byteLength,
    remoteUpdateBytes: update.byteLength,
    localWorkload,
    exportedLocalUpdateBytes,
    exportedSnapshotBytes,
    wasmMiB: wasmBytes / MiB,
    wasmPeakMiB: Math.max(...phaseValues.map((phase) => phase.wasmMiB)),
    localUpdatesWasmGrowthMiB,
    localUpdateExportWasmGrowthMiB,
    snapshotExportWasmGrowthMiB,
    exportedLocalUpdateMiB: exportedLocalUpdateBytes / MiB,
    exportedSnapshotMiB: exportedSnapshotBytes / MiB,
    retainedHeapMiB: retainedHeapBytes / MiB,
    retainedJsonExternalMiB: retainedJsonExternalBytes / MiB,
    retainedJsonRootCount,
    controlledMiB: controlledBytes / MiB,
    rssMiB: usageAfterExport.rss / MiB,
    sampledRssPeakMiB: Math.max(...phaseValues.map((phase) => phase.rssMiB)),
    processMaxRssMiB,
    externalMiB: usageAfterExport.external / MiB,
    budgetMiB,
    underBudget,
    updateRoundTripVersionMatches,
    updateRoundTripVersionEncodingMatches,
    updateRoundTripJsonMatches,
    snapshotRoundTripVersionMatches,
    snapshotRoundTripVersionEncodingMatches,
    snapshotRoundTripJsonMatches,
    phases,
  };
  console.log(JSON.stringify(result));
  if (
    !underBudget ||
    !updateRoundTripVersionMatches ||
    !updateRoundTripJsonMatches ||
    !snapshotRoundTripVersionMatches ||
    !snapshotRoundTripJsonMatches
  ) {
    process.exitCode = 1;
  }
}

function makeUpdates(snapshotPath) {
  const { loro } = loadLoro();
  const snapshot = fs.readFileSync(snapshotPath);
  const suffix = `${process.pid}-${Date.now()}`;
  const peerBase = Number.MAX_SAFE_INTEGER - (Date.now() % 1_000_000);

  const causal = new loro.LoroDoc();
  causal.import(snapshot);
  const baseVersion = causal.version();
  causal.setPeerId(peerBase);
  causal.getMap(`__memory_probe_causal_${suffix}`).set("value", "causal");

  const independent = new loro.LoroDoc();
  independent.setPeerId(peerBase - 1);
  independent
    .getMap(`__memory_probe_independent_${suffix}`)
    .set("value", "independent");

  return [
    {
      scenario: "to-json-then-causal-update",
      update: causal.export({ mode: "update", from: baseVersion }),
    },
    {
      scenario: "to-json-then-independent-update",
      update: independent.export({ mode: "update" }),
    },
  ];
}

function runCoordinator() {
  const firstArg = process.argv[2] === "--" ? 3 : 2;
  const snapshotPath = process.argv[firstArg];
  const budgetMiB = Number(process.argv[firstArg + 1] ?? 100);
  const localUpdateRounds = Number(
    process.argv[firstArg + 2] ?? defaultLocalUpdateRounds,
  );
  const localPayloadBytes = Number(
    process.argv[firstArg + 3] ?? defaultLocalPayloadBytes,
  );
  if (!snapshotPath) {
    die(
      "usage: node --expose-gc scripts/measure-snapshot-round-memory.cjs SNAPSHOT [BUDGET_MIB] [LOCAL_UPDATE_ROUNDS] [LOCAL_PAYLOAD_BYTES]",
    );
  }
  if (!fs.statSync(snapshotPath).isFile()) {
    die(`snapshot is not a file: ${snapshotPath}`);
  }
  if (!Number.isFinite(budgetMiB) || budgetMiB <= 0) {
    die(`invalid budget: ${process.argv[firstArg + 1]}`);
  }
  if (!Number.isSafeInteger(localUpdateRounds) || localUpdateRounds < 0) {
    die(`invalid local update rounds: ${process.argv[firstArg + 2]}`);
  }
  if (!Number.isSafeInteger(localPayloadBytes) || localPayloadBytes <= 0) {
    die(`invalid local payload bytes: ${process.argv[firstArg + 3]}`);
  }

  const results = [];
  let failed = false;
  let scenarioIndex = 0;
  for (const { scenario, update } of makeUpdates(snapshotPath)) {
    const localPeerId = Number.MAX_SAFE_INTEGER - 100_000 - scenarioIndex;
    scenarioIndex += 1;
    const child = spawnSync(
      process.execPath,
      [
        "--expose-gc",
        __filename,
        "--worker",
        snapshotPath,
        scenario,
        Buffer.from(update).toString("base64"),
        String(budgetMiB),
        String(localUpdateRounds),
        String(localPayloadBytes),
        String(localPeerId),
      ],
      { encoding: "utf8", maxBuffer: 4 * MiB },
    );
    if (child.error) {
      throw child.error;
    }
    if (child.stderr) {
      process.stderr.write(child.stderr);
    }
    try {
      results.push(JSON.parse(child.stdout));
    } catch (error) {
      process.stdout.write(child.stdout);
      throw error;
    }
    failed ||= child.status !== 0;
  }

  console.log(
    JSON.stringify(
      {
        metric:
          "input snapshot/remote update buffers + Wasm linear-memory high-water + returned local-update and snapshot Uint8Arrays + retained toJSON heap delta/backing stores",
        budgetMiB,
        localUpdateRounds,
        localPayloadBytes,
        results,
      },
      null,
      2,
    ),
  );
  if (failed) {
    process.exitCode = 1;
  }
}

if (process.argv[2] === "--worker") {
  runWorker();
} else {
  runCoordinator();
}
