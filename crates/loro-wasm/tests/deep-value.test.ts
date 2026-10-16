import { readFileSync } from "node:fs";
import { expect, it } from "vitest";
import { LoroDoc } from "../bundler/index";

// Produced by the Rust issue #1198 reproduction with 20,000 value levels.
function fixture(name: string) {
  return new Uint8Array(
    readFileSync(
      new URL(`../../loro/tests/deep_value_${name}.bin`, import.meta.url),
    ),
  );
}

function assertChain(value: unknown, mixed: boolean) {
  for (let i = 0; i < 20_000; i++) {
    value =
      mixed && i % 2 === 1
        ? (value as unknown[])[0]
        : (value as { a: unknown }).a;
  }
  expect(value).toBe(0);
}

it("reads and frees a small snapshot containing 20,000 nested maps", () => {
  const doc = new LoroDoc();
  doc.import(fixture("map"));
  assertChain(doc.getMap("root").get("v"), false);
  assertChain(doc.toJSON().root.v, false);
  doc.free();
});

it("reads nested map/list values from list and movable-list snapshots", () => {
  const doc = new LoroDoc();
  doc.import(fixture("lists"));
  assertChain(doc.getList("list").get(0), true);
  assertChain(doc.getMovableList("movable").get(0), true);
  doc.free();
});
