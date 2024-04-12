import { configDefaults, defineConfig } from 'vitest/config'
import wasm from "vite-plugin-wasm";

export default defineConfig({
  plugins: [wasm()],
  test: {
    exclude: [
      ...configDefaults.exclude,
      "deno/*"
    ]
  },
});
