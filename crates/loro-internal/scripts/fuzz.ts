import __ from "https://deno.land/x/dirname@1.1.2/mod.ts";
const { __dirname } = __(import.meta);
import { resolve } from "https://deno.land/std@0.105.0/path/mod.ts";

const validTargets = Array.from(
  Deno.readDirSync(resolve(__dirname, "../fuzz/fuzz_targets")),
).map((x) => x.name.replace(/.rs$/, ""));

const targets = Deno.args.length === 0
  ? validTargets
  : Deno.args.filter((x) => validTargets.includes(x));

const promises = [];
for (const target of targets) {
  const cmd = [
    "cargo",
    "+nightly",
    "fuzz",
    "run",
    target,
    "--",
    "-max_total_time=1",
  ];
  console.log("🔨" + cmd.join(" "));
  promises.push(
    Deno.run({
      cmd,
      stdout: "inherit",
      stderr: "inherit",
      cwd: resolve(__dirname, ".."),
    }).status(),
  );
}

await Promise.allSettled(promises);
