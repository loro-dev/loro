// wasm-bindgen emits ES modules for snippets even with --target nodejs.
// Compile them to CommonJS so loading the package never relies on require(esm).
const fs = require("node:fs");
const path = require("node:path");
const { transformSync } = require("esbuild");

function convert(directory) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      convert(file);
    } else if (entry.isFile() && entry.name.endsWith(".js")) {
      const { code } = transformSync(fs.readFileSync(file, "utf8"), {
        format: "cjs",
        target: "node18",
        sourcefile: file,
      });
      fs.writeFileSync(file, code);
    }
  }
}

const directory = process.argv[2];
if (!directory) throw new Error("Expected the nodejs snippets directory");
if (fs.existsSync(directory)) convert(directory);
