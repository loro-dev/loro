import { writeFileSync, writeSync } from "node:fs";
import { createRequire } from "node:module";

import { describe, expect, test } from "vitest";

import {
  ALL_CHECKS,
  ALL_FEATURES,
  CORE_FEATURES,
  describeScenario,
  generateScenario,
  minimizeScenario,
  runScenario,
} from "./fuzz/differential";
import type { FuzzChecks, FuzzFeatures, RustOracle } from "./fuzz/differential";
import { loadRustReference } from "./support/rust-reference";

// See context/loro-js-differential-fuzz.md. Without FUZZ_SEEDS this file runs
// the fixed CI seeds below. Large runs, e.g.:
//
//   FUZZ_SEEDS=0-999 FUZZ_FEATURES=core FUZZ_WASM=1 FUZZ_MINIMIZE=1 \
//     FUZZ_OUT=/tmp/report.json pnpm vitest run tests/differential-fuzz.test.ts
//
// FUZZ_FEATURES: core | all | comma list of FuzzFeatures keys.
// FUZZ_CHECKS:   comma list of FuzzChecks keys (default: all).
// FUZZ_WASM:     1 for the reference in tests/support/rust-reference.ts (honors
//                LORO_WASM_NODEJS), or a path to a loro-crdt nodejs build.
// FUZZ_LOCKSTEP: 0 to compare with Rust only after the scenario.
// FUZZ_ACTIONS:  actions per scenario. FUZZ_TRACE: print actions as they run.
// FUZZ_VALIDATE_HARNESS: check only Rust's own event mirror.
const env = process.env;

const MAP_LIST_TEXT_COUNTER: FuzzFeatures = {
  ...CORE_FEATURES,
  tree: false,
  nested: false,
};

/**
 * Fixed seeds that must pass on every change. They need no WASM build.
 * `pending` seeds fail on main for the reasons listed; move each back into the
 * run once its fix lands.
 */
const CI_RUNS: readonly {
  readonly name: string;
  readonly features: FuzzFeatures;
  readonly checks: FuzzChecks;
  readonly seeds: readonly number[];
  readonly pending: ReadonlyMap<number, string>;
}[] = [
  {
    name: "Map/List/Text/Counter: replay, checkout, snapshot, and event oracles",
    features: MAP_LIST_TEXT_COUNTER,
    checks: { events: true, checkout: true, snapshot: true, shallow: false },
    seeds: Array.from({ length: 120 }, (_, index) => index),
    pending: new Map(),
  },
  {
    name: "MovableList and nested child containers: replay, checkout, snapshot, and event oracles",
    features: { ...MAP_LIST_TEXT_COUNTER, movableList: true, nested: true },
    checks: { events: true, checkout: true, snapshot: true, shallow: false },
    seeds: Array.from({ length: 120 }, (_, index) => index),
    pending: new Map(),
  },
  {
    name: "nested child containers and snapshot imports: replay, checkout, snapshot, and event oracles",
    features: { ...MAP_LIST_TEXT_COUNTER, nested: true, snapshotSync: true },
    checks: { events: true, checkout: true, snapshot: true, shallow: false },
    seeds: Array.from({ length: 120 }, (_, index) => index),
    pending: new Map(),
  },
];

describe.skipIf(env.FUZZ_SEEDS !== undefined)("differential fuzz (fixed seeds)", () => {
  test.each(CI_RUNS)(
    "$name",
    (run) => {
      const failures: string[] = [];
      for (const seed of run.seeds) {
        if (run.pending.has(seed)) continue;
        const scenario = generateScenario(seed, { features: run.features });
        const failure = runScenario(scenario, { checks: run.checks });
        if (failure !== undefined) {
          failures.push(
            `seed ${seed}: ${failure.message}\n${failure.trace.slice(-40).join("\n")}`,
          );
        }
      }
      expect(failures).toEqual([]);
    },
    120_000,
  );

  test("generates the same scenario for a seed", () => {
    expect(describeScenario(generateScenario(7))).toBe(
      describeScenario(generateScenario(7)),
    );
    expect(describeScenario(generateScenario(7))).not.toBe(
      describeScenario(generateScenario(8)),
    );
  });
});

function parseSeeds(input: string): number[] {
  const seeds: number[] = [];
  for (const part of input.split(",")) {
    const [start, end] = part.split("-").map(Number);
    if (end === undefined) seeds.push(start!);
    else for (let seed = start!; seed <= end; seed += 1) seeds.push(seed);
  }
  return seeds;
}

function parseFeatures(input: string | undefined): FuzzFeatures {
  if (input === undefined || input === "core") return CORE_FEATURES;
  if (input === "all") return ALL_FEATURES;
  const enabled = new Set(input.split(","));
  const features: Record<string, boolean> = {};
  for (const key of Object.keys(ALL_FEATURES)) features[key] = enabled.has(key);
  return features as unknown as FuzzFeatures;
}

function parseChecks(input: string | undefined): FuzzChecks {
  if (input === undefined) return ALL_CHECKS;
  const enabled = new Set(input.split(","));
  return {
    events: enabled.has("events"),
    checkout: enabled.has("checkout"),
    snapshot: enabled.has("snapshot"),
    shallow: enabled.has("shallow"),
  };
}

function loadRust(): RustOracle | undefined {
  if (env.FUZZ_WASM === undefined) return undefined;
  const wasm = (
    env.FUZZ_WASM === "1"
      ? loadRustReference()
      : createRequire(import.meta.url)(env.FUZZ_WASM)
  ) as
    | (Record<string, unknown> & {
        LoroDoc: new () => ReturnType<RustOracle["newDoc"]>;
        callPendingEvents(): void;
      })
    | undefined;
  if (wasm === undefined)
    throw new Error("FUZZ_WASM=1 needs the Rust WASM reference build");
  return {
    newDoc: () => new wasm.LoroDoc(),
    newContainer: (type) => new (wasm[`Loro${type}`] as new () => unknown)(),
    flushEvents: () => wasm.callPendingEvents(),
  };
}

describe.runIf(env.FUZZ_SEEDS !== undefined)("differential fuzz (manual)", () => {
  test("runs the requested seeds", () => {
    const seeds = parseSeeds(env.FUZZ_SEEDS!);
    const features = parseFeatures(env.FUZZ_FEATURES);
    const rust = loadRust();
    const checks = parseChecks(env.FUZZ_CHECKS);
    const runOptions = {
      rust,
      checks,
      lockstep: env.FUZZ_LOCKSTEP !== "0",
      validateHarness: env.FUZZ_VALIDATE_HARNESS !== undefined,
      onTrace:
        env.FUZZ_TRACE === undefined
          ? undefined
          : (line: string) => writeSync(2, `${line}\n`),
    };
    const actions = env.FUZZ_ACTIONS === undefined ? undefined : Number(env.FUZZ_ACTIONS);
    const report: unknown[] = [];
    const counts = new Map<string, number>();
    for (const seed of seeds) {
      const scenario = generateScenario(seed, {
        features,
        ...(actions === undefined ? {} : { actions }),
      });
      const failure = runScenario(scenario, runOptions);
      if (failure === undefined) continue;
      counts.set(failure.signature, (counts.get(failure.signature) ?? 0) + 1);
      let minimized = scenario;
      let minimalFailure = failure;
      if (env.FUZZ_MINIMIZE !== undefined) {
        minimized = minimizeScenario(scenario, failure.signature, runOptions);
        minimalFailure = runScenario(minimized, runOptions) ?? failure;
      }
      report.push({
        seed,
        signature: failure.signature,
        message: minimalFailure.message,
        actions: minimized.actions.length,
        trace: minimalFailure.trace,
        scenario: JSON.parse(describeScenario(minimized)),
      });
    }
    const summary = [...counts.entries()].sort((a, b) => b[1] - a[1]);
    if (env.FUZZ_OUT !== undefined) {
      writeFileSync(env.FUZZ_OUT, JSON.stringify({ summary, report }, null, 2));
    }
    expect({ failures: report.length, summary }).toEqual({ failures: 0, summary: [] });
  }, 3_600_000);
});
