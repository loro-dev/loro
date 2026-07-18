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
    include: ["tests/**/*.test.ts"],
  },
});
