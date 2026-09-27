import { readFileSync, writeFileSync } from "node:fs";

import { describe, expect, test } from "vitest";

import {
  fuzz,
  fuzzScale,
  minimizeTrace,
  replayTrace,
  type FuzzProfile,
  type SavedTrace,
} from "./fuzz";
import { loadLoroJs, loadReference } from "./reference";

/**
 * Randomized differential tests of MovableList (and its child containers)
 * against the Rust implementation. See context/loro-js-movable-list.md.
 *
 * Profiles leave out behavior that diverges for reasons unrelated to the
 * MovableList model; they are listed in that article under "Remaining known
 * divergences". In short: undo is only exercised on a single peer, and
 * checkout never targets the current frontiers.
 */

const rust = loadReference();
const engines = { rust: rust!, js: loadLoroJs() };

const none: FuzzProfile = {
  name: "",
  peers: 3,
  moves: false,
  children: false,
  undo: false,
  history: false,
  snapshots: false,
  shallow: false,
  crossEngine: true,
  metadata: true,
  events: true,
};

interface Case {
  readonly name: string;
  readonly profile: FuzzProfile;
  readonly seeds: number;
  readonly steps: number;
}

const cases: Case[] = [
  {
    name: "insert/delete",
    profile: { ...none, name: "insert/delete", children: true, history: true },
    seeds: 40,
    steps: 80,
  },
  {
    name: "insert/delete undo (one peer)",
    profile: { ...none, name: "insert/delete undo (one peer)", peers: 1, undo: true },
    seeds: 40,
    steps: 60,
  },
  {
    name: "move/set",
    profile: { ...none, name: "move/set", moves: true },
    seeds: 40,
    steps: 80,
  },
  {
    name: "move/set with children",
    profile: { ...none, name: "move/set with children", moves: true, children: true },
    seeds: 30,
    steps: 80,
  },
  {
    name: "move/set history",
    profile: { ...none, name: "move/set history", moves: true, history: true },
    seeds: 30,
    steps: 80,
  },
  {
    name: "move/set undo (one peer)",
    profile: {
      ...none,
      name: "move/set undo (one peer)",
      peers: 1,
      moves: true,
      undo: true,
    },
    seeds: 30,
    steps: 60,
  },
  {
    name: "snapshots",
    profile: { ...none, name: "snapshots", moves: true, children: true, snapshots: true },
    seeds: 30,
    steps: 80,
  },
  {
    name: "shallow snapshots",
    profile: {
      ...none,
      name: "shallow snapshots",
      moves: true,
      history: true,
      snapshots: true,
      shallow: true,
    },
    seeds: 30,
    steps: 80,
  },
];

/**
 * Profiles that still diverge because loro.js models a MovableList element as
 * an item that moves inside the Fugue sequence. The Rust-compatible model in
 * context/loro-js-movable-list.md removes them from this set.
 */
const KNOWN_DIVERGENT = new Set<string>([]);

describe.skipIf(rust === undefined)("MovableList matches the Rust implementation", () => {
  test.each(cases)(
    "$name",
    ({ name, profile, seeds, steps }) => {
      const scale = fuzzScale(seeds, steps);
      const summary = fuzz(engines, profile, scale.seeds, scale.steps);
      const expected = KNOWN_DIVERGENT.has(name) ? "diverges" : "converges";
      const first = summary.failures[0];
      expect({
        outcome: first === undefined ? "converges" : "diverges",
        // Show the first divergence of a profile that should converge.
        detail:
          expected === "converges" && first !== undefined
            ? `${summary.failures.length}/${summary.seeds} seeds diverged\n${first.message}`
            : undefined,
      }).toEqual({ outcome: expected, detail: undefined });
    },
    600_000,
  );
});

// Debugging aid: LORO_JS_DIFF_REPLAY=<saved trace> replays one failing trace,
// and LORO_JS_DIFF_MINIMIZE=1 first shrinks it to a minimal failing sequence.
const replayPath = process.env["LORO_JS_DIFF_REPLAY"];
test.runIf(replayPath !== undefined && rust !== undefined)(
  "replays a saved trace",
  () => {
    let trace = JSON.parse(readFileSync(replayPath!, "utf8")) as SavedTrace;
    if (process.env["LORO_JS_DIFF_MINIMIZE"] === "1") {
      trace = minimizeTrace(engines, trace);
      writeFileSync(`${replayPath!}.min.json`, JSON.stringify(trace));
    }
    const message = replayTrace(engines, trace);
    expect(
      message === undefined
        ? []
        : [message, trace.actions.map((action) => JSON.stringify(action)).join("\n")],
    ).toEqual([]);
  },
  600_000,
);
