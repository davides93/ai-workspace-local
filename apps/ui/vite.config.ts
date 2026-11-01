import { defineConfig } from "vite";
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
