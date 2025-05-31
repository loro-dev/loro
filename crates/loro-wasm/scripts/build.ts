import * as path from "https://deno.land/std@0.105.0/path/mod.ts";
import { gzip } from "https://deno.land/x/compress@v0.4.5/mod.ts";
import brotliPromise from "npm:brotli-wasm";
import { getOctokit } from "npm:@actions/github";

// Polyfill for missing performance.markResourceTiming function in Deno
if (typeof performance !== 'undefined' && !(performance as any).markResourceTiming) {
  (performance as any).markResourceTiming = () => { };
}

const __dirname = path.dirname(path.fromFileUrl(import.meta.url));

// deno run -A build.ts debug
// deno run -A build.ts release
// deno run -A build.ts release web
// deno run -A build.ts release nodejs
let profile = "dev";
let profileDir = "debug";
if (Deno.args[0] == "release") {
  profile = "release";
  profileDir = "release";
}
const TARGETS = ["bundler", "nodejs", "web"];
const startTime = performance.now();
const LoroWasmDir = path.resolve(__dirname, "..");

// Check if running in CI
const isCI = Deno.env.get("CI") === "true";
const githubToken = Deno.env.get("GITHUB_TOKEN");
const githubEventPath = Deno.env.get("GITHUB_EVENT_PATH");

console.log({
  isCI,
  githubToken: !!githubToken,
  githubEventPath: githubEventPath,
});
async function build() {
  await cargoBuild();
  const target = Deno.args[1];
  if (target != null) {
    if (!TARGETS.includes(target)) {
      throw new Error(`Invalid target ${target}`);
    }

    buildTarget(target);
    return;
  }

  await Promise.all(
    TARGETS.map((target) => {
      return buildTarget(target);
    }),
  );

  if (profile !== "dev") {
    await Promise.all(
      TARGETS.map(async (target) => {
        // --snip-rust-panicking-code --snip-rust-fmt-code
        // const snip = `wasm-snip ./${target}/loro_wasm_bg.wasm -o ./${target}/loro_wasm_bg.wasm`;
        // console.log(">", snip);
        // await Deno.run({ cmd: snip.split(" "), cwd: LoroWasmDir }).status();
        // const cmd = `wasm-opt -O4 ./${target}/loro_wasm_bg.wasm -o ./${target}/loro_wasm_bg.wasm`;
        // console.log(">", cmd);
        // await Deno.run({ cmd: cmd.split(" "), cwd: LoroWasmDir }).status();
      }),
    );
  }

  console.log("\n");
  console.log(
    "✅",
    "Build complete in",
    (performance.now() - startTime) / 1000,
    "s",
  );

  if (profile === "release") {
    const wasm = await Deno.readFile(
      path.resolve(LoroWasmDir, "bundler", "loro_wasm_bg.wasm"),
    );
    const wasmSize = (wasm.length / 1024).toFixed(2);
    console.log("Wasm size: ", wasmSize, "KB");

    const gzipped = await gzip(wasm);
    const gzipSize = (gzipped.length / 1024).toFixed(2);
    console.log("Gzipped size: ", gzipSize, "KB");

    // Use brotli-wasm for brotli compression
    const brotli = await brotliPromise;
    const brotliCompressed = brotli.compress(wasm);
    const brotliSize = (brotliCompressed.length / 1024).toFixed(2);
    console.log("Brotli size: ", brotliSize, "KB");

    // Report sizes to PR if in CI
    if (isCI && githubToken && githubEventPath) {
      console.log("Creating comment for PR");
      try {
        // Parse GitHub event data
        const event = JSON.parse(await Deno.readTextFile(githubEventPath));
        console.log("event", event);
        if (event.pull_request) {
          const prNumber = event.pull_request.number;
          const repo = event.repository.full_name;
          const [owner, repoName] = repo.split("/");

          const commentBody = `## WASM Size Report
<!-- loro-wasm-size-report -->
- Original size: ${wasmSize} KB
- Gzipped size: ${gzipSize} KB
- Brotli size: ${brotliSize} KB`;

          // Initialize Octokit client
          const octokit = getOctokit(githubToken);

          // Find if we already have a comment with our marker
          const { data: comments } = await octokit.rest.issues.listComments({
            owner,
            repo: repoName,
            issue_number: prNumber,
          });

          const sizeReportMarker = "<!-- loro-wasm-size-report -->";
          const existingComment = comments.find((comment) =>
            comment.body?.includes(sizeReportMarker)
          );

          if (existingComment) {
            // Update existing comment
            await octokit.rest.issues.updateComment({
              owner,
              repo: repoName,
              comment_id: existingComment.id,
              body: commentBody,
            });
            console.log("Updated existing WASM size report comment");
          } else {
            // Create new comment
            await octokit.rest.issues.createComment({
              owner,
              repo: repoName,
              issue_number: prNumber,
              body: commentBody,
            });
            console.log("Created new WASM size report comment");
          }
        }
      } catch (error) {
        console.error("Failed to report sizes to PR:", error);
      }
    }
  }
}

async function cargoBuild() {
  const cmd =
    `cargo build --target wasm32-unknown-unknown --profile ${profile}`;
  console.log(cmd);
  const status = await Deno.run({
    cmd: cmd.split(" "),
    cwd: LoroWasmDir,
  }).status();
  if (!status.success) {
    console.log(
      "❌",
      "Build failed in",
      (performance.now() - startTime) / 1000,
      "s",
    );
    Deno.exit(status.code);
  }
}

async function buildTarget(target: string) {
  console.log("🏗️  Building target", `[${target}]`);
  const targetDirPath = path.resolve(LoroWasmDir, target);
  try {
    await Deno.remove(targetDirPath, { recursive: true });
    console.log("Clear directory " + targetDirPath);
  } catch (_e) {
    //
  }

  // TODO: polyfill FinalizationRegistry
  const cmd =
    `wasm-bindgen --weak-refs --target ${target} --out-dir ${target} ../../target/wasm32-unknown-unknown/${profileDir}/loro_wasm.wasm`;
  console.log(">", cmd);
  await Deno.run({ cmd: cmd.split(" "), cwd: LoroWasmDir }).status();
  console.log();

  if (target === "nodejs") {
    console.log("🔨  Patching nodejs target");
    const patch = await Deno.readTextFile(
      path.resolve(__dirname, "./nodejs_patch.js"),
    );
    const wasm = await Deno.readTextFile(
      path.resolve(targetDirPath, "loro_wasm.js"),
    );
    await Deno.writeTextFile(
      path.resolve(targetDirPath, "loro_wasm.js"),
      wasm + "\n" + patch,
    );
  }
  if (target === "bundler") {
    console.log("🔨  Patching bundler target");
    const patch = await Deno.readTextFile(
      path.resolve(__dirname, "./bundler_patch.js"),
    );
    await Deno.writeTextFile(
      path.resolve(targetDirPath, "loro_wasm.js"),
      patch,
    );
  }
}

build();
