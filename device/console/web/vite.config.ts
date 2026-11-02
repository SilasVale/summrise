import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { viteSingleFile } from "vite-plugin-singlefile";

// One file out: the console embeds this page with `include_str!`, so everything (React, CSS, the
// brand mark) has to live inside index.html. That is the whole reason this is a build step rather
// than a hand-written page.
export default defineConfig({
  plugins: [react(), viteSingleFile()],
  // es2022 for the wasm logic seam: its module initializes once at import time with top-level
  // await, and a component must never render against half-loaded logic. WebView2 and every
  // supported browser are Chromium/modern; es2020 could not express it.
  build: { target: "es2022", assetsInlineLimit: 1024 * 1024, cssCodeSplit: false }
});
