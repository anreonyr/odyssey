import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";

// Vite config — keeps the dev server simple, lets the Rust bridge
// own the production routing. `server.proxy` is what makes `pnpm dev`
// usable: the React app talks to relative paths, Vite forwards
// /api/* to the Rust server on :3030.
//
// Vite emits `<script type="module" crossorigin>` for the prod
// bundle. jsdom (the test harness) does not execute module
// scripts — the React mount never fires, and the test sees an
// empty `<div id="root">`. The `stripModuleScriptType` plugin
// rewrites the emitted HTML to a plain `<script>` tag so jsdom
// can drive it. Real browsers ignore the type and run it the
// same way.
function stripModuleScriptType(): Plugin {
  return {
    name: "strip-module-script-type",
    apply: "build",
    transformIndexHtml: {
      order: "post",
      handler(html) {
        return html.replace(/<script type="module" /g, "<script ");
      },
    },
  };
}

export default defineConfig({
  plugins: [react(), stripModuleScriptType()],
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: "http://127.0.0.1:3030",
        changeOrigin: false,
      },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: true,
    target: "es2020",
    rollupOptions: {
      output: {
        // IIFE + inlineDynamicImports keeps the bundle as a
        // single file so the test can fetch it via a single
        // <script> tag.
        format: "iife",
        inlineDynamicImports: true,
      },
    },
  },
});
