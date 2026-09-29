// Headless render smoke test: execute the built bundle in jsdom and verify
// the app mounts (login page renders when /api/me 401s). Usage:
//   node render-smoke.mjs <path-to-built-js>
import { JSDOM } from "jsdom";
import { readFileSync } from "node:fs";

const jsPath = process.argv[2];
if (!jsPath) { console.error("usage: node render-smoke.mjs <built-js>"); process.exit(1); }
const html = readFileSync("../public/index.html", "utf8");
const js = readFileSync(jsPath, "utf8");

const dom = new JSDOM(html, {
  url: "https://ai.saisi.online/",
  runScripts: "outside-only",
  pretendToBeVisual: true,
});
const { window } = dom;
// THE CONSOLE'S RUST, COMPILED FROM THE SERVED FILE. jsdom has no server to fetch
// `/ui_logic_bg.wasm` from, and since 2026-09-29 the migrated functions are called DURING RENDER —
// so without this the smoke renders a page whose marks throw. It is the SAME artifact the browser
// gets (`../public/ui_logic_bg.wasm`, the file vite copies out of `ui/public/`), compiled here
// instead of streamed, which is the seam's own "two environments, one artifact" rule.
// AND JSDOM HAS NO `WebAssembly` AT ALL, which is why the shim beside it is not decoration: without
// it `initSync`'s `module instanceof WebAssembly.Module` throws inside the seam, the load promise
// rejects, and the page renders with `logic()` throwing on its first call. The browser has it; the
// harness must say the same thing about the environment it is standing in for.
window.WebAssembly = WebAssembly;
window.__consoleLogicModule = WebAssembly.compile(readFileSync("../public/ui_logic_bg.wasm"));
window.fetch = async () =>
  new Response(JSON.stringify({ type: "error", error: { message: "unauthorized" } }), { status: 401 });

// jsdom eval runs as classic script — shim import.meta (harness-only).
window.__ims = (s) => s;
const shimmed = js
  .replaceAll("import.meta.resolve", "window.__ims")
  .replaceAll("import.meta.url", JSON.stringify("https://ai.saisi.online/"));
try {
  window.eval(shimmed);
} catch (e) {
  console.error("BUNDLE THREW:", e.message);
  process.exit(1);
}
await new Promise((r) => setTimeout(r, 400));
const root = window.document.getElementById("root");
const text = root?.textContent || "";
console.log("root children:", root?.children.length);
console.log("content sample:", JSON.stringify(text.slice(0, 100)));
if (root?.children.length > 0 && text.includes("Summrise")) { console.log("RENDER OK"); } else { console.error("RENDER FAILED"); process.exit(1); }
