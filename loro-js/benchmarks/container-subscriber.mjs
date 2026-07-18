/* eslint-disable no-console */

import { performance } from "node:perf_hooks";

import { LoroDoc } from "../dist/index.js";

const positionalArguments = process.argv.slice(2).filter((argument) => argument !== "--");
const sizes = (positionalArguments[0] ?? "1000,8000,64000").split(",").map(Number);
const iterations = Number(positionalArguments[1] ?? 100);

for (const size of sizes) {
  const doc = new LoroDoc();
  const roots = Array.from({ length: size }, (_, index) => doc.getMap(`root-${index}`));
  const subscriptions = roots.map((root) => root.subscribe(() => {}));
  const target = roots.at(-1);
  if (target === undefined)
    throw new RangeError("subscriber benchmark size must be positive");

  target.set("value", -1);
  doc.commit();
  globalThis.gc?.();
  const start = performance.now();
  for (let iteration = 0; iteration < iterations; iteration += 1) {
    target.set("value", iteration);
    doc.commit();
  }
  const milliseconds = performance.now() - start;
  console.log(
    JSON.stringify({
      subscribers: size,
      iterations,
      milliseconds,
      millisecondsPerCommit: milliseconds / iterations,
    }),
  );

  for (const unsubscribe of subscriptions) unsubscribe();
}
