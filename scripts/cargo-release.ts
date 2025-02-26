#!/usr/bin/env -S deno run --allow-run

import { defineCommand, runMain } from "npm:citty";
import { runCheckLoroVersion } from "./check-loro-version.ts";

async function runCargoRelease(version: string): Promise<string> {
  const process = new Deno.Command("cargo", {
    args: ["release", "version", "--workspace", version],
  });
  const output = await process.output();
  return new TextDecoder().decode(output.stderr);
}

function parseNoChangesCrates(output: string): string[] {
  const lines = output.split("\n");
  const noChangesCrates: string[] = [];

  for (const line of lines) {
    if (line.includes("despite no changes made since tag")) {
      const match = line.match(/updating ([^ ]+) to/);
      if (match) {
        noChangesCrates.push(match[1]);
      }
    }
  }

  return noChangesCrates;
}

function generateOptimizedCommand(
  version: string,
  excludedCrates: string[],
): string {
  const excludeFlags = excludedCrates.map((crate) => `--exclude ${crate}`).join(
    " ",
  );
  return `cargo release version --workspace ${version} ${excludeFlags}`;
}

function isValidVersion(version: string): boolean {
  // Matches format like 1.2.3
  return /^\d+\.\d+\.\d+$/.test(version);
}

const main = defineCommand({
  meta: {
    name: "cargo-release",
    version: "1.0.0",
    description: "Bump version with optimized excludes",
  },
  args: {
    version: {
      type: "positional",
      description: "Version to bump to (format: x.y.z)",
      required: true,
    },
  },
  async run({ args }) {
    const version = args.version;
    console.log(version);

    if (!isValidVersion(version)) {
      throw new Error("Version must be in format x.y.z (e.g., 1.2.3)");
    }

    runCheckLoroVersion(version);
    const output = await runCargoRelease(version);
    console.log("Original output:");
    console.log(output);

    const noChangesCrates = parseNoChangesCrates(output);
    if (noChangesCrates.length > 0) {
      console.log("\nOptimized command:");
      console.log(generateOptimizedCommand(version, noChangesCrates));
    }
  },
});

runMain(main);
