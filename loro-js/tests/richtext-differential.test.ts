import { describe, expect, test } from "vitest";

import { generateActions, runActions } from "./support/richtext-differential";
import { loadRustReference } from "./support/rust-reference";

const rust = loadRustReference();
const seeds = Number(process.env.LORO_DIFF_SEEDS ?? 40);
const steps = Number(process.env.LORO_DIFF_STEPS ?? 80);
const firstSeed = Number(process.env.LORO_DIFF_FIRST_SEED ?? 1);
// Marks need loro.js to keep Rust's style anchors in the text sequence.
const options = { marks: false };

describe.skipIf(rust === undefined)("text differential against Rust", () => {
  test.each(Array.from({ length: seeds }, (_, index) => firstSeed + index))(
    "random concurrent text edits converge with Rust (seed %i)",
    (seed) => {
      expect(runActions(rust!, generateActions(seed, steps, options), options)).toBe(
        undefined,
      );
    },
  );
});
