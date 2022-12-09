// deno run -A build.ts debug
// deno run -A build.ts release
// deno run -A build.ts release web
// deno run -A build.ts release nodejs
let profile = "dev"
if (Deno.args[0] == "release") {
  profile = "release"
}
const TARGETS = ["bundler", "web", "nodejs"];
const startTime = performance.now();

async function build() {
  await cargoBuild();
  if (Deno.args[1] != null) {
    if (!TARGETS.includes(Deno.args[1])) {
      throw new Error(`Invalid target ${Deno.args[1]}`);
    }

    buildTarget(Deno.args[1]);
    return;
  }

  await Promise.all(TARGETS.map(target => {
    return buildTarget(target);
  }));

  console.log(
    "✅",
    "Build complete in",
    (performance.now() - startTime) / 1000,
    "s",
  );
}

async function cargoBuild() {
  const status = await Deno.run({
    cmd: `cargo build --target wasm32-unknown-unknown --profile release`
      .split(" "),
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
  for (const cmd of genCommands(target)) {
    console.log(">", cmd);
    await Deno.run({ cmd: cmd.split(" ") }).status();
  }
  console.log()
}

function genCommands(target: string): string[] {
  return [
    `rm -rf ./${target}`,
    `wasm-bindgen --weak-refs --target ${target} --out-dir ${target} ../../target/wasm32-unknown-unknown/release/loro_wasm.wasm`,
    ...(profile == "dev" ? [] : [`wasm-opt -O4 ${target}/loro_wasm_bg.wasm -o ${target}/loro_wasm_bg.wasm`]),
  ];
}

build();
