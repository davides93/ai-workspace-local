import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { viteSingleFile } from "vite-plugin-singlefile";

// `--mode mockup` produces one self-contained HTML file (demo transport, no backend)
// that is published as the interactive mockup. Other modes build the app UI that is
// served by `aiw serve` and embedded by the Tauri shell.
export default defineConfig(({ mode }) => ({
  plugins: [react(), ...(mode === "mockup" ? [viteSingleFile()] : [])],
  define: {
    __AIW_FORCE_MOCK__: JSON.stringify(mode === "mockup"),
  },
  build: {
    outDir: mode === "mockup" ? "dist-mockup" : "dist",
    emptyOutDir: true,
    target: "es2022",
  },
  test: {
    setupFiles: ["src/test-setup.ts"],
    // tokens.test.ts reads tokens.css (and the OLED overrides in oled.css) as text to check the contrast of the palette.
    css: { include: [/tokens\.css/, /oled\.css/] },
    // Two kinds of test: the pure logic (`*.test.ts`, no DOM) and the views (`*.test.tsx`, rendered in jsdom against the
    // demo transport; see `src/test-utils`).
    projects: [
      { extends: true, test: { name: "logic", environment: "node", include: ["src/**/*.test.ts"] } },
      {
        extends: true,
        test: {
          name: "components",
          environment: "jsdom",
          include: ["src/**/*.test.tsx"],
          setupFiles: ["src/test-utils/setup-dom.ts"],
          // The first test of a file also loads (and transforms) the lazy chunk of its view.
          testTimeout: 20_000,
        },
      },
    ],
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": { target: "http://127.0.0.1:7777", changeOrigin: true },
      "/v1": { target: "http://127.0.0.1:7777", changeOrigin: true },
    },
  },
  clearScreen: false,
}));
