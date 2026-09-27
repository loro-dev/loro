import { describe, expect, test } from "vitest";

import { generateActions, runActions } from "./support/richtext-differential";
import { loadRustReference } from "./support/rust-reference";

const rust = loadRustReference();
const seeds = Number(process.env.LORO_DIFF_SEEDS ?? 40);
const steps = Number(process.env.LORO_DIFF_STEPS ?? 80);
const firstSeed = Number(process.env.LORO_DIFF_FIRST_SEED ?? 1);

describe.skipIf(rust === undefined)("rich text differential against Rust", () => {
  test.each(Array.from({ length: seeds }, (_, index) => firstSeed + index))(
    "random concurrent rich-text edits converge with Rust (seed %i)",
    (seed) => {
      expect(runActions(rust!, generateActions(seed, steps))).toBeUndefined();
    },
  );
  // Plain text keeps its own seeds so a rich-text failure does not hide one.
  test.each(Array.from({ length: seeds >> 1 }, (_, index) => firstSeed + index))(
    "random concurrent plain-text edits converge with Rust (seed %i)",
    (seed) => {
      const options = { marks: false };
      expect(runActions(rust!, generateActions(seed, steps, options), options)).toBe(
        undefined,
      );
    },
  );
});
