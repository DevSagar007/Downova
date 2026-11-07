/// <reference types="vitest/config" />
import fs from "node:fs";
import path from "node:path";
import { defineConfig, type Plugin } from "vite";
import { buildManifest, type Target } from "./manifest";

// Build targets (ADR 0029), chosen explicitly with `--mode`:
//   production (default) → Chromium in dist/          e2e         → Chromium with test hooks
//   firefox              → Firefox in dist-firefox/   firefox-e2e → Firefox with test hooks
// Each target has its own output directory, so building one never erases the other. The e2e modes
// add the real-browser test hooks and the permissions automation cannot grant; normal builds never
// contain either (scripts/package.mjs checks).
function targetOf(mode: string): { target: Target; e2e: boolean } {
  return { target: mode.startsWith("firefox") ? "firefox" : "chromium", e2e: mode.endsWith("e2e") };
}

/** Writes the target's manifest.json into the output once the bundle is complete. */
function manifest(target: Target, e2e: boolean): Plugin {
  let outDir = "dist";
  return {
    name: "downova-manifest",
    apply: "build",
    configResolved(config) {
      outDir = config.build.outDir;
    },
    closeBundle() {
      fs.writeFileSync(
        path.resolve(outDir, "manifest.json"),
        JSON.stringify(buildManifest(target, e2e), null, 2) + "\n",
      );
    },
  };
}

export default defineConfig(({ mode }) => {
  const { target, e2e } = targetOf(mode);
  return {
    plugins: [manifest(target, e2e)],
    define: {
      __DOWNOVA_TARGET__: JSON.stringify(target),
      __DOWNOVA_E2E__: JSON.stringify(e2e),
    },
    build: {
      target: "es2022",
      outDir: target === "firefox" ? "dist-firefox" : "dist",
      emptyOutDir: true,
      modulePreload: false,
      rollupOptions: {
        input: {
          background: "src/background.ts",
          popup: "popup.html",
          options: "options.html",
          // Reconnecting a signed-in download is a Chromium-only flow (BRW-05).
          ...(target === "chromium" ? { reconnect: "reconnect.html" } : {}),
        },
        output: {
          entryFileNames: "[name].js",
          chunkFileNames: "chunks/[name].js",
          assetFileNames: "assets/[name][extname]",
        },
      },
    },
    // `css: true` lets tests read stylesheets with `?raw` (contrast checks, B2.14).
    test: { environment: "node", css: true },
  };
});
