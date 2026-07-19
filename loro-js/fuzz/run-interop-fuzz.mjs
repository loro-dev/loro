import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import fc from "fast-check";

import {
  canonicalize,
  runDifferentialScenario,
  runMalformedImportChecks,
  runSingleScenario,
} from "./runner.mjs";
import { assertScenario, scenarioArbitrary } from "./scenario.mjs";

const fuzzRoot = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(fuzzRoot, "..");
const workspaceRoot = path.resolve(packageRoot, "..");
const jsEntry = path.join(packageRoot, "dist/index.js");
const wasmEntries = [
  path.join(workspaceRoot, "crates/loro-wasm/nodejs/index.js"),
  path.join(workspaceRoot, "crates/loro-wasm/nodejs/loro_wasm.js"),
];
const corpusRoot = path.join(fuzzRoot, "corpus");
const strictCorpusRoot = path.join(corpusRoot, "strict");
const artifactsRoot = path.join(fuzzRoot, "artifacts");

const options = parseArgs(process.argv.slice(2));
requireBuild(jsEntry, "pnpm --filter loro-js build");
const wasmEntry = wasmEntries.find(existsSync);
if (wasmEntry === undefined) {
  throw new Error(`missing Rust/WASM package; run pnpm release-wasm first`);
}

const jsEngine = await import(pathToFileURL(jsEntry).href);
const importedWasmEngine = await import(pathToFileURL(wasmEntry).href);
const wasmEngine =
  importedWasmEngine.LoroDoc === undefined
    ? importedWasmEngine.default
    : importedWasmEngine;
const corpus = readCorpus(options.replay, options.strict);
const malformedCases = runMalformedImportChecks(wasmEngine, jsEngine);

for (const { name, scenario } of corpus) {
  runDifferentialScenario(wasmEngine, jsEngine, scenario, {
    strict: options.strict,
  });
  if (process.env.LORO_INTEROP_NATIVE_DRIVER !== undefined) {
    verifyNative(name, scenario, jsEngine, process.env.LORO_INTEROP_NATIVE_DRIVER);
  }
}

if (!options.corpusOnly) {
  const arbitrary = scenarioArbitrary(options.maxCommands, { strict: options.strict });
  const property = fc.property(arbitrary, (scenario) => {
    runDifferentialScenario(wasmEngine, jsEngine, scenario, {
      strict: options.strict,
    });
  });
  const check = fc.check(property, {
    numRuns: options.runs,
    seed: options.seed,
    path: options.path,
    endOnFailure: true,
    interruptAfterTimeLimit: options.timeLimit,
  });
  if (check.failed) {
    mkdirSync(artifactsRoot, { recursive: true });
    const scenario = check.counterexample?.[0];
    const artifact = path.join(artifactsRoot, `failure-${Date.now()}.json`);
    if (scenario !== undefined) {
      writeFileSync(artifact, `${JSON.stringify(scenario, null, 2)}\n`);
    }
    const details = [
      `Loro interop fuzz failed after ${check.numRuns} runs`,
      `seed=${check.seed}`,
      `path=${check.counterexamplePath ?? ""}`,
      `artifact=${artifact}`,
      check.errorInstance?.stack ?? check.error ?? "unknown error",
    ].join("\n");
    throw new Error(details);
  }
}

process.stdout.write(
  `loro interop fuzz passed: profile=${options.strict ? "strict" : "stable"} corpus=${corpus.length} malformed=${malformedCases} random=${options.corpusOnly ? 0 : options.runs}\n`,
);

function parseArgs(args) {
  const ci = args.includes("--ci");
  const corpusOnly = args.includes("--corpus-only");
  const strict = args.includes("--strict");
  const replayIndex = args.indexOf("--replay");
  const replay = replayIndex >= 0 ? args[replayIndex + 1] : undefined;
  const envRuns = Number.parseInt(process.env.LORO_INTEROP_FUZZ_RUNS ?? "", 10);
  const envCommands = Number.parseInt(
    process.env.LORO_INTEROP_FUZZ_MAX_COMMANDS ?? "",
    10,
  );
  const envSeed = Number.parseInt(process.env.LORO_INTEROP_FUZZ_SEED ?? "", 10);
  const envTime = Number.parseInt(process.env.LORO_INTEROP_FUZZ_TIME_MS ?? "", 10);
  return {
    ci,
    corpusOnly,
    strict,
    replay,
    runs: Number.isSafeInteger(envRuns) ? envRuns : ci ? 500 : 100,
    maxCommands: Number.isSafeInteger(envCommands) ? envCommands : ci ? 60 : 40,
    seed: Number.isSafeInteger(envSeed) ? envSeed : undefined,
    path: process.env.LORO_INTEROP_FUZZ_PATH,
    timeLimit: Number.isSafeInteger(envTime) ? envTime : ci ? 60_000 : undefined,
  };
}

function requireBuild(entry, command) {
  if (!existsSync(entry)) {
    throw new Error(`missing ${entry}; run ${command} first`);
  }
}

function readCorpus(replay, strict) {
  const files =
    replay === undefined
      ? [...jsonFiles(corpusRoot), ...(strict ? jsonFiles(strictCorpusRoot) : [])]
      : [path.resolve(replay)];
  return files.map((file) => ({
    name: path.basename(file),
    scenario: assertScenario(JSON.parse(readFileSync(file, "utf8"))),
  }));
}

function jsonFiles(root) {
  return readdirSync(root)
    .filter((name) => name.endsWith(".json"))
    .sort()
    .map((name) => path.join(root, name));
}

function verifyNative(name, scenario, js, driver) {
  const jsSelf = runSingleScenario(js, scenario);
  const nativeSelf = callNative(driver, { scenario });
  compareNative(`${name}: native self`, nativeSelf.observations, jsSelf.observations);

  const nativeFromJs = callNative(driver, {
    scenario,
    externalBlobs: jsSelf.transportBlobs,
  });
  compareNative(
    `${name}: JS -> native Rust`,
    nativeFromJs.observations,
    jsSelf.observations,
  );

  const jsFromNative = runSingleScenario(js, scenario, nativeSelf.transportBlobs);
  compareNative(
    `${name}: native Rust -> JS`,
    jsFromNative.observations,
    nativeSelf.observations,
  );
}

function callNative(driver, input) {
  const result = spawnSync(driver, [], {
    input: JSON.stringify(input),
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.status !== 0) {
    throw new Error(`native driver failed: ${result.stderr || result.stdout}`);
  }
  return JSON.parse(result.stdout);
}

function compareNative(label, left, right) {
  const canonicalLeft = canonicalize(left);
  const canonicalRight = canonicalize(right);
  if (JSON.stringify(canonicalLeft) !== JSON.stringify(canonicalRight)) {
    throw new Error(
      `${label} mismatch\nnative=${JSON.stringify(canonicalLeft)}\njs=${JSON.stringify(canonicalRight)}`,
    );
  }
}
