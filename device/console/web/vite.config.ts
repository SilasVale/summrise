import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { viteSingleFile } from "vite-plugin-singlefile";

// One file out: the console embeds this page with `include_str!`, so everything (React, CSS, the
// brand mark) has to live inside index.html. That is the whole reason this is a build step rather
// than a hand-written page.
export default defineConfig({
  plugins: [react(), viteSingleFile()],
  build: { target: "es2020", assetsInlineLimit: 1024 * 1024, cssCodeSplit: false }
});
