#!/usr/bin/env node
import { spawn } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const workspaceRoot = path.resolve(__dirname, "..");
const fuzzRoot = path.join(workspaceRoot, "crates/fuzz");

const args = process.argv.slice(2);
if (args.length === 0 || args.includes("-h") || args.includes("--help")) {
  console.log(`Usage: node scripts/cargo-fuzz-run.mjs <target> [corpus...] [-- <libFuzzer args...>]

Environment:
  LORO_FUZZ_SANITIZER=auto     Use platform default (default)
  LORO_FUZZ_SANITIZER=address  Force ASan
  LORO_FUZZ_SANITIZER=none     Disable sanitizer

On macOS arm64, auto disables ASan because the current Rust nightly ASan
runtime can spin during process initialization before the fuzz target runs.`);
  process.exit(args.length === 0 ? 1 : 0);
}

const envSanitizer = process.env.LORO_FUZZ_SANITIZER ?? "auto";
const sanitizer = resolveSanitizer(envSanitizer);
const cargoArgs = ["+nightly", "fuzz", "run"];
if (sanitizer) {
  cargoArgs.push("-s", sanitizer);
}
cargoArgs.push(...args);

if (sanitizer === "none") {
  console.error(
    "cargo-fuzz: using sanitizer=none (set LORO_FUZZ_SANITIZER=address to force ASan)",
  );
}

const child = spawn("cargo", cargoArgs, {
  cwd: fuzzRoot,
  env: process.env,
  stdio: "inherit",
});

child.on("close", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});

child.on("error", (error) => {
  console.error(error.message);
  process.exit(1);
});

function resolveSanitizer(value) {
  switch (value) {
    case "":
    case "auto":
      return shouldDisableAsanByDefault() ? "none" : "";
    case "address":
    case "leak":
    case "memory":
    case "thread":
    case "none":
      return value;
    default:
      throw new Error(
        `Invalid LORO_FUZZ_SANITIZER=${value}. Expected auto, address, leak, memory, thread, or none.`,
      );
  }
}

function shouldDisableAsanByDefault() {
  return os.platform() === "darwin" && os.arch() === "arm64";
}
