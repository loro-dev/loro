/// <reference types="vitest" />
import wasm from "vite-plugin-wasm";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [wasm()],
  // test: {},
});
