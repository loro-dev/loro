import { execFile as execFileCb } from "node:child_process";
import {
  cp,
  mkdir,
  readdir,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const execFile = promisify(execFileCb);

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const packageDir = path.resolve(__dirname, "..");
const repoRoot = path.resolve(packageDir, "../..");
const tmpRoot = path.join(packageDir, ".tmp");
const localLoroPackage = path.join(repoRoot, "crates/loro-wasm");
const loroPackageSpec = normalizeLoroPackageSpec(
  process.env.LORO_SMOKE_PACKAGE ?? `file:${localLoroPackage}`,
);
const smokeMode = process.env.LORO_SMOKE_MODE ?? "default";

function normalizeLoroPackageSpec(spec) {
  if (spec === "loro-crdt" || spec.startsWith("loro-crdt@")) {
    return `npm:${spec}`;
  }

  return spec;
}

const sharedApp = (importPath) => {
  if (smokeMode === "json") {
    return `import { LoroDoc } from "${importPath}";

const doc = new LoroDoc();
doc.getText("t").insert(0, "hi");
const value = doc.toJSON();

if (value.t !== "hi" || Object.keys(value).length !== 1) {
  throw new Error(\`Unexpected Loro JSON: \${JSON.stringify(value)}\`);
}

console.log(value);
globalThis.__LORO_JSON_SMOKE__ = value;
const app = document.getElementById("app");
if (app) {
  app.textContent = JSON.stringify(value);
}
`;
  }

  return `import { LoroDoc } from "${importPath}";

const doc = new LoroDoc();
const text = doc.getText("text");
text.insert(0, "bundler-smoke");

const value = doc.toJSON().text;
if (value !== "bundler-smoke") {
  throw new Error(\`Unexpected Loro value: \${value}\`);
}

globalThis.__LORO_SMOKE__ = value;
const app = document.getElementById("app");
if (app) {
  app.textContent = value;
}
`;
};

const html = `<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Loro bundler smoke</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.js"></script>
  </body>
</html>
`;

const cases = {
  vite5: viteCase("vite5", "vite", "^5.4.21"),
  vite6: viteCase("vite6", "vite", "^6.4.2"),
  vite7: viteCase("vite7", "vite", "^7.3.2"),
  vite8: viteCase("vite8", "vite", "^8.0.10"),
  "vite5-dev": viteDevCase("vite5-dev", "^5.4.21"),
  "vite6-dev": viteDevCase("vite6-dev", "^6.4.2"),
  "vite7-dev": viteDevCase("vite7-dev", "^7.3.2"),
  "vite8-dev": viteDevCase("vite8-dev", "^8.0.10"),
  "vite5-web-mirror-dev": viteDevCase("vite5-web-mirror-dev", "^5.4.21", {
    setup: setupViteWebMirror,
    dependencies: { "loro-mirror": "^2.1.1" },
  }),
  "rolldown-vite": viteCase("rolldown-vite", "rolldown-vite", "^7.3.1"),
  "rolldown-vite-dev": viteDevCase("rolldown-vite-dev", "^7.3.1", {
    packageName: "rolldown-vite",
  }),
  webpack5: {
    description: "Webpack 5 production build",
    dependencies: {},
    devDependencies: {
      webpack: "^5.106.2",
      "webpack-cli": "^6.0.1",
    },
    setup: setupWebpack,
    command: ["pnpm", "exec", "webpack", "--config", "webpack.config.cjs"],
    inspect: { dir: "dist", wasmAsset: true, noWasmWrapper: true },
  },
  "webpack5-dev": {
    description: "Webpack 5 dev server runtime",
    dependencies: {},
    devDependencies: {
      webpack: "^5.106.2",
      "webpack-cli": "^6.0.1",
      "webpack-dev-server": "^5.2.2",
    },
    setup: setupWebpack,
    command: ["pnpm", "exec", "webpack", "--config", "webpack.config.cjs"],
    inspect: { dir: "dist", wasmAsset: true, noWasmWrapper: true },
  },
  rsbuild2: {
    description: "Rsbuild 2 production build",
    dependencies: {},
    devDependencies: {
      "@rsbuild/core": "^2.0.3",
    },
    setup: setupRsbuild,
    command: ["pnpm", "exec", "rsbuild", "build"],
    inspect: { dir: "dist", wasmAsset: true, noWasmWrapper: true },
  },
  "rsbuild2-dev": {
    description: "Rsbuild 2 dev server runtime",
    dependencies: {},
    devDependencies: {
      "@rsbuild/core": "^2.0.3",
    },
    setup: setupRsbuild,
    command: ["pnpm", "exec", "rsbuild", "build"],
    inspect: { dir: "dist", wasmAsset: true, noWasmWrapper: true },
  },
  rspack2: {
    description: "Rspack 2 production build",
    dependencies: {},
    devDependencies: {
      "@rspack/cli": "^2.0.1",
      "@rspack/core": "^2.0.1",
    },
    setup: setupRspack,
    command: ["pnpm", "exec", "rspack", "build", "--config", "rspack.config.cjs"],
    inspect: { dir: "dist", wasmAsset: true, noWasmWrapper: true },
  },
  "rspack2-dev": {
    description: "Rspack 2 dev server runtime",
    dependencies: {},
    devDependencies: {
      "@rspack/cli": "^2.0.1",
      "@rspack/core": "^2.0.1",
      "@rspack/dev-server": "^2.0.1",
    },
    setup: setupRspack,
    command: ["pnpm", "exec", "rspack", "build", "--config", "rspack.config.cjs"],
    inspect: { dir: "dist", wasmAsset: true, noWasmWrapper: true },
  },
  parcel2: {
    description: "Parcel 2 production build",
    dependencies: {},
    devDependencies: {
      parcel: "^2.16.4",
    },
    setup: setupParcel,
    command: [
      "pnpm",
      "exec",
      "parcel",
      "build",
      "index.html",
      "--dist-dir",
      "dist",
      "--no-cache",
    ],
    inspect: { dir: "dist", wasmAsset: true, noWasmWrapper: true },
  },
  "parcel2-dev": {
    description: "Parcel 2 dev server runtime",
    dependencies: {},
    devDependencies: {
      parcel: "^2.16.4",
    },
    setup: setupParcel,
    command: [
      "pnpm",
      "exec",
      "parcel",
      "build",
      "index.html",
      "--dist-dir",
      "dist",
      "--no-cache",
    ],
    inspect: { dir: "dist", wasmAsset: true, noWasmWrapper: true },
  },
  "esbuild-default-copy": {
    description: "esbuild browser build with explicit WASM copy",
    dependencies: {},
    devDependencies: {
      esbuild: "^0.28.0",
    },
    setup: (dir) => setupBasic(dir, "loro-crdt"),
    command: [
      "pnpm",
      "exec",
      "esbuild",
      "src/main.js",
      "--bundle",
      "--format=esm",
      "--platform=browser",
      "--outdir=dist",
    ],
    afterBuild: (dir) => copyBrowserWasm(dir, "dist/loro_wasm_bg.wasm"),
    inspect: { dir: "dist", wasmAsset: true, noWasmWrapper: true },
  },
  "esbuild-base64": {
    description: "esbuild browser build with base64 entry",
    dependencies: {},
    devDependencies: {
      esbuild: "^0.28.0",
    },
    setup: (dir) => setupBasic(dir, "loro-crdt/base64"),
    command: [
      "pnpm",
      "exec",
      "esbuild",
      "src/main.js",
      "--bundle",
      "--format=esm",
      "--platform=browser",
      "--outdir=dist",
    ],
    inspect: { dir: "dist", noWasmAsset: true, noWasmWrapper: true },
  },
  "rollup-default-copy": {
    description: "Rollup 4 browser build with explicit WASM copy",
    dependencies: {},
    devDependencies: {
      rollup: "^4.60.2",
      "@rollup/plugin-node-resolve": "^16.0.3",
    },
    setup: (dir) => setupRollup(dir, "loro-crdt"),
    command: ["pnpm", "exec", "rollup", "-c", "rollup.config.mjs"],
    afterBuild: (dir) => copyBrowserWasm(dir, "dist/loro_wasm_bg.wasm"),
    inspect: { dir: "dist", wasmAsset: true, noWasmWrapper: true },
  },
  "rollup-base64": {
    description: "Rollup 4 browser build with base64 entry",
    dependencies: {},
    devDependencies: {
      rollup: "^4.60.2",
      "@rollup/plugin-node-resolve": "^16.0.3",
    },
    setup: (dir) => setupRollup(dir, "loro-crdt/base64"),
    command: ["pnpm", "exec", "rollup", "-c", "rollup.config.mjs"],
    inspect: { dir: "dist", noWasmAsset: true, noWasmWrapper: true },
  },
  "next16-turbopack": {
    description: "Next 16 production build with default Turbopack",
    dependencies: {
      next: "^16.2.4",
      react: "^19.2.1",
      "react-dom": "^19.2.1",
    },
    devDependencies: {},
    setup: setupNext,
    command: ["pnpm", "exec", "next", "build"],
    inspect: { dir: ".next", noWasmWrapper: true },
  },
  "next16-webpack": {
    description: "Next 16 production build with Webpack and base64 entry",
    dependencies: {
      next: "^16.2.4",
      react: "^19.2.1",
      "react-dom": "^19.2.1",
    },
    devDependencies: {},
    setup: (dir) => setupNext(dir, "loro-crdt/base64"),
    command: ["pnpm", "exec", "next", "build", "--webpack"],
    inspect: { dir: ".next", noWasmAsset: true, noWasmWrapper: true },
  },
  "next16-turbopack-dev": {
    description: "Next 16 dev server runtime with default Turbopack",
    dependencies: {
      next: "^16.2.4",
      react: "^19.2.1",
      "react-dom": "^19.2.1",
    },
    devDependencies: {},
    setup: setupNext,
    command: ["pnpm", "exec", "next", "build"],
    inspect: { dir: ".next", noWasmWrapper: true },
  },
  "next16-webpack-dev": {
    description: "Next 16 dev server runtime with Webpack and base64 entry",
    dependencies: {
      next: "^16.2.4",
      react: "^19.2.1",
      "react-dom": "^19.2.1",
    },
    devDependencies: {},
    setup: (dir) => setupNext(dir, "loro-crdt/base64"),
    command: ["pnpm", "exec", "next", "build", "--webpack"],
    inspect: { dir: ".next", noWasmAsset: true, noWasmWrapper: true },
  },
};

function viteCase(name, packageName, version) {
  return {
    description: `${name} production build`,
    dependencies: {},
    devDependencies: {
      [packageName]: version,
      typescript: "^5.9.3",
    },
    setup: setupVite,
    command: ["pnpm", "exec", "vite", "build"],
    inspect: { dir: "dist", wasmAsset: true, noWasmWrapper: true },
  };
}

function viteDevCase(name, version, options = {}) {
  const packageName = options.packageName ?? "vite";
  return {
    description: `${name} dev server runtime`,
    dependencies: options.dependencies ?? {},
    devDependencies: {
      [packageName]: version,
      "vite-plugin-wasm": "^3.6.0",
      "vite-plugin-top-level-await": "^1.6.0",
      rollup: "^4.60.2",
      esbuild: "^0.27.7",
      typescript: "^5.9.3",
    },
    setup: options.setup ?? setupViteDev,
    command: ["pnpm", "exec", "vite", "build"],
    inspect: { dir: "dist", wasmAsset: true },
  };
}

async function setupBasic(dir, importPath) {
  await mkdir(path.join(dir, "src"), { recursive: true });
  await writeFile(path.join(dir, "index.html"), html);
  await writeFile(path.join(dir, "src/main.js"), sharedApp(importPath));
}

async function setupVite(dir) {
  await setupBasic(dir, "loro-crdt");
  await writeFile(
    path.join(dir, "vite.config.js"),
    `export default { build: { outDir: "dist" } };\n`,
  );
}

async function setupViteDev(dir) {
  await setupBasic(dir, "loro-crdt");
  await writeViteWasmConfig(dir);
}

async function setupViteWebMirror(dir) {
  await mkdir(path.join(dir, "src"), { recursive: true });
  await writeFile(path.join(dir, "index.html"), html);
  await writeFile(
    path.join(dir, "src/main.js"),
    `import init, { LoroDoc } from "loro-crdt/web";
import wasmUrl from "loro-crdt/web/loro_wasm_bg.wasm?url";
import * as mirror from "loro-mirror";

await init(wasmUrl);
const doc = new LoroDoc();
doc.getText("t").insert(0, "hi");
const value = doc.toJSON();

if (value.t !== "hi" || Object.keys(value).length !== 1) {
  throw new Error(\`Unexpected Loro JSON: \${JSON.stringify(value)}\`);
}

globalThis.__LORO_JSON_SMOKE__ = value;
globalThis.__LORO_MIRROR_KEYS__ = Object.keys(mirror);
const app = document.getElementById("app");
if (app) {
  app.textContent = JSON.stringify(value);
}
`,
  );
  await writeViteWasmConfig(dir);
}

async function writeViteWasmConfig(dir) {
  await writeFile(
    path.join(dir, "vite.config.js"),
    `import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";

export default {
  plugins: [wasm(), topLevelAwait()],
  build: { outDir: "dist", target: "esnext" },
};
`,
  );
}

async function setupWebpack(dir) {
  await setupBasic(dir, "loro-crdt");
  await writeBundleHtml(dir);
  await writeFile(
    path.join(dir, "webpack.config.cjs"),
    `const path = require("node:path");

module.exports = {
  mode: "production",
  entry: "./src/main.js",
  output: {
    path: path.resolve(__dirname, "dist"),
    filename: "bundle.js",
    clean: true,
  },
};
`,
  );
}

async function setupRspack(dir) {
  await setupBasic(dir, "loro-crdt");
  await writeBundleHtml(dir);
  await writeFile(
    path.join(dir, "rspack.config.cjs"),
    `const path = require("node:path");

module.exports = {
  mode: "production",
  entry: "./src/main.js",
  output: {
    path: path.resolve(__dirname, "dist"),
    filename: "bundle.js",
    clean: true,
  },
};
`,
  );
}

async function writeBundleHtml(dir) {
  await mkdir(path.join(dir, "public"), { recursive: true });
  await writeFile(
    path.join(dir, "public/index.html"),
    `<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Loro bundler smoke</title>
  </head>
  <body>
    <div id="app"></div>
    <script src="/bundle.js"></script>
  </body>
</html>
`,
  );
}

async function setupRsbuild(dir) {
  await setupBasic(dir, "loro-crdt");
  await writeFile(
    path.join(dir, "index.html"),
    `<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Loro bundler smoke</title>
  </head>
  <body>
    <div id="app"></div>
  </body>
</html>
`,
  );
  await writeFile(
    path.join(dir, "rsbuild.config.mjs"),
    `export default {
  source: { entry: { index: "./src/main.js" } },
  html: { template: "./index.html" },
  output: { distPath: { root: "dist" } },
};
`,
  );
}

async function setupParcel(dir) {
  await mkdir(path.join(dir, "src"), { recursive: true });
  await writeFile(
    path.join(dir, "index.html"),
    html.replace("/src/main.js", "./src/main.js"),
  );
  await writeFile(path.join(dir, "src/main.js"), sharedApp("loro-crdt"));
}

async function setupRollup(dir, importPath) {
  await setupBasic(dir, importPath);
  await writeFile(
    path.join(dir, "rollup.config.mjs"),
    `import nodeResolve from "@rollup/plugin-node-resolve";

export default {
  input: "./src/main.js",
  output: { dir: "dist", format: "esm" },
  plugins: [nodeResolve({ browser: true })],
};
`,
  );
}

async function setupNext(dir, importPath = "loro-crdt") {
  await mkdir(path.join(dir, "app"), { recursive: true });
  await mkdir(path.join(dir, "components"), { recursive: true });
  await writeFile(
    path.join(dir, "app/layout.jsx"),
    `export default function RootLayout({ children }) {
  return <html><body>{children}</body></html>;
}
`,
  );
  await writeFile(
    path.join(dir, "app/page.jsx"),
    `"use client";

import Smoke from "../components/Smoke.jsx";

export default function Page() {
  return <Smoke />;
}
`,
  );
  await writeFile(
    path.join(dir, "components/Smoke.jsx"),
    smokeMode === "json"
      ? `"use client";

import { LoroDoc } from "${importPath}";

export default function Smoke() {
  const doc = new LoroDoc();
  doc.getText("t").insert(0, "hi");
  const value = doc.toJSON();

  if (value.t !== "hi" || Object.keys(value).length !== 1) {
    throw new Error(\`Unexpected Loro JSON: \${JSON.stringify(value)}\`);
  }

  console.log(value);
  globalThis.__LORO_JSON_SMOKE__ = value;
  return <main>{JSON.stringify(value)}</main>;
}
`
      : `"use client";

import { LoroDoc } from "${importPath}";

export default function Smoke() {
  const doc = new LoroDoc();
  const text = doc.getText("text");
  text.insert(0, "bundler-smoke");
  return <main>{doc.toJSON().text}</main>;
}
`,
  );
  await writeFile(
    path.join(dir, "next.config.mjs"),
    `export default {
  typescript: { ignoreBuildErrors: true },
};
`,
  );
}

async function copyBrowserWasm(dir, outputRelativePath) {
  const source = path.join(
    dir,
    "node_modules/loro-crdt/browser/loro_wasm_bg.wasm",
  );
  const target = path.join(dir, outputRelativePath);
  await mkdir(path.dirname(target), { recursive: true });
  await cp(source, target);
}

async function writePackageJson(dir, testCase) {
  const pkg = {
    private: true,
    type: "module",
    scripts: {},
    dependencies: {
      "loro-crdt": loroPackageSpec,
      ...testCase.dependencies,
    },
    devDependencies: testCase.devDependencies,
  };

  await writeFile(
    path.join(dir, "package.json"),
    `${JSON.stringify(pkg, null, 2)}\n`,
  );
}

async function runCommand(command, cwd) {
  const [bin, ...args] = command;
  try {
    const { stdout, stderr } = await execFile(bin, args, {
      cwd,
      env: {
        ...process.env,
        NEXT_TELEMETRY_DISABLED: "1",
      },
      maxBuffer: 1024 * 1024 * 16,
    });
    if (stdout.trim()) {
      process.stdout.write(stdout);
    }
    if (stderr.trim()) {
      process.stderr.write(stderr);
    }
  } catch (error) {
    if (error.stdout) {
      process.stdout.write(error.stdout);
    }
    if (error.stderr) {
      process.stderr.write(error.stderr);
    }
    throw error;
  }
}

async function install(dir) {
  await runCommand(
    ["pnpm", "install", "--ignore-workspace", "--prefer-offline"],
    dir,
  );
}

async function runCase(name, testCase) {
  const dir = path.join(tmpRoot, name);
  await rm(dir, { recursive: true, force: true });
  await mkdir(dir, { recursive: true });
  await writePackageJson(dir, testCase);
  await testCase.setup(dir);

  console.log(`\n[${name}] ${testCase.description}`);
  await install(dir);
  await runCommand(testCase.command, dir);
  if (testCase.afterBuild) {
    await testCase.afterBuild(dir);
  }
  await inspectOutput(dir, testCase.inspect);
  console.log(`[${name}] ok`);
}

async function inspectOutput(dir, inspect) {
  const outputDir = path.join(dir, inspect.dir);
  await assertExists(outputDir, `Expected output directory ${inspect.dir}`);
  const files = await listFiles(outputDir);
  const wasmFiles = files.filter((file) => file.endsWith(".wasm"));
  const wasmWrapperFiles = files.filter((file) =>
    /(^|[/\\])loro_wasm_bg(?:[-.][^/\\]+)?\.js$/.test(file)
  );

  if (inspect.wasmAsset && wasmFiles.length === 0) {
    throw new Error(`Expected a emitted .wasm asset in ${inspect.dir}`);
  }

  if (inspect.noWasmAsset && wasmFiles.length > 0) {
    throw new Error(
      `Did not expect .wasm assets in ${inspect.dir}: ${wasmFiles.join(", ")}`,
    );
  }

  if (inspect.noWasmWrapper && wasmWrapperFiles.length > 0) {
    throw new Error(
      `Unexpected wasm wrapper chunk(s): ${wasmWrapperFiles.join(", ")}`,
    );
  }
}

async function listFiles(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...await listFiles(fullPath));
    } else if (entry.isFile()) {
      files.push(fullPath);
    }
  }
  return files;
}

async function assertExists(filePath, message) {
  try {
    await stat(filePath);
  } catch {
    throw new Error(message);
  }
}

async function ensureLocalPackageArtifacts() {
  if (process.env.LORO_SMOKE_PACKAGE) {
    return;
  }

  const required = [
    "bundler/index.js",
    "browser/loro_wasm.js",
    "browser/loro_wasm_bg.wasm",
    "base64/index.js",
    "nodejs/index.js",
    "web/index.js",
  ];

  const missing = [];
  for (const file of required) {
    try {
      await stat(path.join(localLoroPackage, file));
    } catch {
      missing.push(file);
    }
  }

  if (missing.length > 0) {
    throw new Error(
      `Local loro-crdt build artifacts are missing: ${missing.join(", ")}.\n` +
        "Run `pnpm release-wasm` first, or set LORO_SMOKE_PACKAGE to a published package spec.",
    );
  }
}

function listCases() {
  for (const [name, testCase] of Object.entries(cases)) {
    console.log(`${name.padEnd(22)} ${testCase.description}`);
  }
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes("--list")) {
    listCases();
    return;
  }

  await ensureLocalPackageArtifacts();
  const selected = args.length > 0 ? args : Object.keys(cases);
  const unknown = selected.filter((name) => !cases[name]);
  if (unknown.length > 0) {
    throw new Error(`Unknown smoke case(s): ${unknown.join(", ")}`);
  }

  for (const name of selected) {
    await runCase(name, cases[name]);
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
