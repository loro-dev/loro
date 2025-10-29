import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";
import { viteWasmDebug } from "../../plugins/vite-wasm-debug";

const workspaceRoot = fileURLToPath(new URL("../../", import.meta.url));

export default defineConfig({
  plugins: [
    vue(),
    wasm(),
    topLevelAwait(),
    viteWasmDebug({ readSourcesFromDisk: false }),
  ],
  build: {
    rollupOptions: {
      output: {
        assetFileNames: (assetInfo) => {
          if (assetInfo.name?.endsWith(".wasm.map")) {
            return "assets/[name][extname]";
          }
          if (assetInfo.name?.endsWith(".debug.wasm")) {
            return "assets/[name][extname]";
          }
          return "assets/[name]-[hash][extname]";
        },
      },
    },
  },
  server: {
    fs: {
      allow: [workspaceRoot],
    },
  },
});
