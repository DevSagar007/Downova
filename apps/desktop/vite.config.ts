/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";

// The version shown in About: the build version (scripts/package.mjs sets DOWNOVA_BUILD_VERSION
// for fixture builds), otherwise the Tauri config version, so it always matches the package.
const appVersion =
  process.env.DOWNOVA_BUILD_VERSION ??
  (
    JSON.parse(readFileSync(new URL("./src-tauri/tauri.conf.json", import.meta.url), "utf8")) as {
      version: string;
    }
  ).version;

// Tauri expects a fixed dev port and must see Rust-side errors in the terminal.
export default defineConfig({
  plugins: [react()],
  define: { __DOWNOVA_VERSION__: JSON.stringify(appVersion) },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    target: "es2022",
    sourcemap: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    css: true,
    // Whole-window tests type and click through dialogs; on a loaded machine (a release build
    // running, a shared CI runner) one can exceed the 5 s default without anything being wrong.
    testTimeout: 20_000,
  },
});
