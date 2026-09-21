import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Desktop-only (Windows / WebView2). See docs/05-architecture-and-optimisation.md §7.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
    watch: { ignored: ["**/src-tauri/**", "**/docs/**", "**/tools/**"] },
  },
  build: {
    // WebView2 is Chromium 153: no down-levelling, no polyfills. Vite 8 minifies with Oxc (esbuild is not bundled).
    target: "esnext",
    minify: "oxc",
    sourcemap: false,
    cssCodeSplit: false,
    reportCompressedSize: true,
    chunkSizeWarningLimit: 200,
  },
});
