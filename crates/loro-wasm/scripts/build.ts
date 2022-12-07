// deno run -A build.ts debug
// deno run -A build.ts release
// deno run -A build.ts release web
// deno run -A build.ts release nodejs
let profile = "dev";
let target_dir = "debug";

switch (Deno.args[0]) {
  case "debug":
    break;
  case "release":
    profile = "release";
    target_dir = "release";
    break;
}

const TARGETS = ["bundler", "web", "nodejs"];
const startTime = performance.now();

async function build() {
  await cargoBuild();
  if (Deno.args[1] != null) {
    if (!TARGETS.includes(Deno.args[1])) {
      throw new Error(`Invalid target ${Deno.args[1]}`);
    }

    for (const cmd of genCommands(Deno.args[1])) {
      await Deno.run({ cmd: cmd.split(" ") }).status();
    }
    return;
  }

  for (const target of TARGETS) {
    await buildTarget(target);
  }

  console.log(
    "✅",
    "Build complete in",
    (performance.now() - startTime) / 1000,
    "s",
  );
}

async function cargoBuild() {
  await Deno.run({
    cmd: `cargo build --target wasm32-unknown-unknown --profile ${profile}`
      .split(" "),
  }).status();
}

async function buildTarget(target: string) {
  console.log("🏗️  Building target", `[${target}]`);
  for (const cmd of genCommands(target)) {
    console.log(">", cmd);
    await Deno.run({ cmd: cmd.split(" ") }).status();
  }
  console.log()
}

function genCommands(target: string): string[] {
  return [
    `rm -rf ./${target}`,
    `wasm-bindgen --weak-refs --target ${target} --out-dir ${target} ../../target/wasm32-unknown-unknown/${target_dir}/loro_wasm.wasm`,
    ...(profile == "dev" ? [] : [`wasm-opt -O4 ${target}/loro_wasm_bg.wasm -o ${target}/loro_wasm_bg.wasm`]),
  ];
}

build();
