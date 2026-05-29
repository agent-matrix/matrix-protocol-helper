import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Vite config for the MatrixHub Client frontend.
// Tauri drives this via beforeDevCommand / beforeBuildCommand.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  // Tauri expects a fixed port and fails if it is not available.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      // Don't watch the Rust side.
      ignored: ["**/src-tauri/**"],
    },
  },
  // Produce a relative-path bundle so it loads from tauri://localhost.
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: ["es2021", "chrome105", "safari13"],
    sourcemap: false,
  },
});
