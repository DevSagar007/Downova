/// <reference types="vitest/config" />
import { defineConfig } from "vite";

// MV3 extension: the background service worker and the popup are separate entry points.
// `public/manifest.json` is copied verbatim; output names are stable because the manifest
// refers to them.
export default defineConfig({
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
    modulePreload: false,
    rollupOptions: {
      input: { background: "src/background.ts", popup: "popup.html" },
      output: {
        entryFileNames: "[name].js",
        chunkFileNames: "chunks/[name].js",
        assetFileNames: "assets/[name][extname]",
      },
    },
  },
  test: { environment: "node" },
});
