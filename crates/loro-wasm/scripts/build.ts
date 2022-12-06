import { build, emptyDir } from "https://deno.land/x/dnt@0.32.0/mod.ts";

async function main() {
  await emptyDir("./npm");

  await build({
    entryPoints: ["./mod.ts"],
    outDir: "./npm",
    shims: {
      // see JS docs for overview and more options
      deno: true
    },
    test: false,
    package: {
      // package.json properties
      name: "loro-wasm",
      version: "0.0.1",
      description: "",
      license: "MIT",
      repository: {
        type: "git",
        url: "git+https://github.com/loro-dev/loro.git",
      },
      bugs: {
        url: "https://github.com/loro-dev/loro/issues",
      },
    },
    packageManager: "pnpm"
  });

  // post build steps
  await Deno.copyFile("LICENSE", "npm/LICENSE");
  await Deno.copyFile("README.md", "npm/README.md");
  await Deno.copyFile("pkg/loro_wasm_bg.wasm", "npm/esm/pkg/loro_wasm_bg.wasm");
  await Deno.copyFile("pkg/loro_wasm_bg.wasm", "npm/script/pkg/loro_wasm_bg.wasm");
}

main();
