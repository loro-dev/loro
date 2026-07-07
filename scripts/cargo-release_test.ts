import {
  parseReleasePlan,
  selectExcludedCrates,
} from "./cargo-release-plan.ts";

Deno.test("keeps no-change crates when a required dependency is upgraded", () => {
  const output = `
warning: updating loro-delta to 1.13.7 despite no changes made since tag loro-delta-v1.13.0
warning: updating loro-rle to 1.13.7 despite no changes made since tag loro-rle-v1.6.0
   Upgrading generic-btree from 0.10.7 to 1.13.7
    Updating loro-delta's dependency from 0.10.7 to 1.13.7
   Upgrading loro-delta from 1.13.0 to 1.13.7
   Upgrading loro-rle from 1.6.0 to 1.13.7
`;

  const excludedCrates = selectExcludedCrates(parseReleasePlan(output));

  if (excludedCrates.includes("loro-delta")) {
    throw new Error(
      "loro-delta should be kept so its generic-btree dependency can be bumped",
    );
  }
  if (!excludedCrates.includes("loro-rle")) {
    throw new Error("unrelated no-change crates should still be excluded");
  }
});
