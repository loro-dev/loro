import { defineConfig } from "vite-plus";

export default defineConfig({
  fmt: {
    ignorePatterns: ["dist/**", "node_modules/**", "tests/fixtures/**"],
    printWidth: 90,
    semi: true,
    singleQuote: false,
    sortPackageJson: false,
    trailingComma: "all",
  },
  lint: {
    ignorePatterns: ["dist/**", "node_modules/**", "tests/fixtures/**"],
    options: {
      denyWarnings: true,
      reportUnusedDisableDirectives: "error",
      typeAware: true,
      typeCheck: true,
    },
    plugins: ["typescript", "oxc", "import", "vitest"],
    rules: {
      "no-console": "error",
    },
  },
  pack: {
    clean: true,
    dts: true,
    entry: ["src/index.ts", "src/codec/index.ts"],
    format: "esm",
    outDir: "dist",
    platform: "neutral",
    sourcemap: true,
    target: "es2022",
    tsconfig: "tsconfig.build.json",
  },
  test: {
    environment: "node",
    // The differential suite needs a WASM build of the Rust implementation; run it
    // with `pnpm test:differential` after `pnpm build:reference`.
    include:
      process.env["LORO_JS_DIFFERENTIAL"] === "1"
        ? ["tests/differential/**/*.test.ts"]
        : ["tests/**/*.test.ts"],
    exclude:
      process.env["LORO_JS_DIFFERENTIAL"] === "1"
        ? ["**/node_modules/**"]
        : ["**/node_modules/**", "tests/differential/**"],
  },
});
