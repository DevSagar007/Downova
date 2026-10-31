/// <reference types="vitest/config" />
import fs from "node:fs";
import path from "node:path";
import { defineConfig, type Plugin } from "vite";

// Real-browser tests only (`--mode e2e`): Chrome asks for optional permissions in its own
// prompt, which automation cannot answer, so the e2e build declares the capture permissions as
// granted (and only for the local fixture hosts). Normal builds keep them optional.
function e2eManifest(mode: string): Plugin {
  let outDir = "dist";
  return {
    name: "downova-e2e-manifest",
    apply: "build",
    configResolved(config) {
      outDir = config.build.outDir;
    },
    // `public/` is copied after the bundle, so the written file is edited at the very end.
    closeBundle() {
      if (mode !== "e2e") return;
      const file = path.resolve(outDir, "manifest.json");
      const m = JSON.parse(fs.readFileSync(file, "utf8"));
      m.permissions = [...m.permissions, ...m.optional_permissions];
      m.host_permissions = ["http://127.0.0.1/*", "http://localhost/*"];
      delete m.optional_permissions;
      fs.writeFileSync(file, JSON.stringify(m, null, 2));
    },
  };
}

export default defineConfig(({ mode }) => ({
  plugins: [e2eManifest(mode)],
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
    modulePreload: false,
    rollupOptions: {
      input: { background: "src/background.ts", popup: "popup.html", options: "options.html" },
      output: {
        entryFileNames: "[name].js",
        chunkFileNames: "chunks/[name].js",
        assetFileNames: "assets/[name][extname]",
      },
    },
  },
  // `css: true` lets tests read stylesheets with `?raw` (contrast checks, B2.14).
  test: { environment: "node", css: true },
}));
