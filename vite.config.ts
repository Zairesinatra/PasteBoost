import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  // Packaged Tauri pages load bundled assets from an embedded URL, so keep
  // production references relative to index.html instead of the URL root.
  base: "./",
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
