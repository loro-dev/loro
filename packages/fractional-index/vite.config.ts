import { defineConfig } from "vite-plus";

export default defineConfig({
  fmt: {
    ignorePatterns: ["dist/**", "node_modules/**"],
    printWidth: 90,
    semi: true,
    singleQuote: false,
    sortPackageJson: false,
    trailingComma: "all",
  },
  lint: {
    ignorePatterns: ["dist/**", "node_modules/**"],
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
    entry: "src/index.ts",
    format: "esm",
    outDir: "dist",
    platform: "neutral",
    sourcemap: true,
    target: "es2020",
    tsconfig: "tsconfig.build.json",
  },
  test: {
    environment: "node",
    include: ["tests/**/*.test.ts"],
  },
});
