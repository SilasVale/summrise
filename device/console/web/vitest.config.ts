import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// The tests run the components, not the single-file bundle: `vite.config.ts`'s plugin exists to
// inline everything into the one html the binary embeds, and applying it here would only slow a test
// run down. jsdom is the browser the page's own DOM code assumes; nothing in the suite needs a real
// layout.
export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    restoreMocks: true
  }
});
