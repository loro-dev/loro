// Builds the Node target of `crates/loro-wasm` (the `loro-crdt` package) in the
// dev profile, for `pnpm test:differential`. This is the subset of
// `pnpm -C crates/loro-wasm build-dev` that the differential suite needs: one
// target, no bundler/browser outputs, and no package tests.
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const wasmDir = fileURLToPath(new URL("../../crates/loro-wasm/", import.meta.url));

function run(command, args) {
  const result = spawnSync(command, args, { cwd: wasmDir, stdio: "inherit" });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} exited with ${result.status}`);
  }
}

run("deno", ["run", "-A", "./scripts/build.ts", "dev", "nodejs"]);
run("pnpm", ["exec", "rollup", "-c"]);

// Same rewrite as `crates/loro-wasm/scripts/post-rollup.ts` for the Node target.
const indexPath = new URL("nodejs/index.js", `file://${wasmDir}`);
const index = readFileSync(indexPath, "utf8").replace(
  /require\(["']loro-wasm["']\)/g,
  'require("./loro_wasm")',
);
writeFileSync(indexPath, index);
