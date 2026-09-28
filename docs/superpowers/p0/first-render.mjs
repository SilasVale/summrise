// P0's first-render baseline: FCP for the panel and the console, from THIS checkout's built bundles.
//
//   node docs/superpowers/p0/first-render.mjs panel   [runs]
//   node docs/superpowers/p0/first-render.mjs console [runs]
//
// NO LISTENER IS OPENED. Every request is answered by Playwright route interception, which is the
// same trick agent/scripts/panel-render-audit.mjs and gateway/ui/scripts/screenshot.mjs already use —
// so this is safe on a box where binding a port outside 127.0.0.1 is not allowed.
//
// THE PANEL IS MEASURED ON THE REPOSITORY'S OWN HARNESS, in its URL mode, so panel.js and panel.css
// are FETCHED (production shape — a wasm module would be a separate fetch) while the stubbed device
// API is the one panel-render-audit.mjs already ships. Emit it first, with no browser helper set:
//
//   SUMMRISE_PANEL_BUNDLE_URL=http://summrise.test/panel \
//     node agent/scripts/panel-render-audit.mjs --out /tmp/p0     # exits 2: that is its emit path
//
// The console is measured against its own built bundle with /api/* answering 401, which is
// render-smoke.mjs's own assumption: the login page is what renders.
//
// NEEDS: a Chromium for playwright-core, and the shared libraries for it. The measured recipe is in
// agent/resources/panel-react/scripts/local-browser.mjs (`apt-get download` needs no root):
//   export LD_LIBRARY_PATH=/tmp/chromium-libs/prefix/usr/lib/x86_64-linux-gnu
//   export SUMMRISE_CHROMIUM_PATH=~/.cache/ms-playwright/chromium-1228/chrome-linux64/chrome
// The bundles must be built first: `npm run build` in each UI directory.
import { chromium } from "playwright-core";
import { readFileSync, existsSync } from "node:fs";
import { join, extname, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const MIME = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css",
  ".svg": "image/svg+xml", ".png": "image/png", ".json": "application/json", ".ico": "image/x-icon",
  // P2 ADDED THIS ENTRY, and the instrument is where it belongs: since the panel's logic moved to
  // Rust the page fetches `panel_logic_bg.wasm` at its first migrated call, and serving it as
  // anything but this makes the wasm-bindgen glue fall back to an ArrayBuffer with a console
  // warning — measured, and INVISIBLE to an FCP number, which is exactly why the harness should not
  // be the part that is wrong. It cannot move the baseline: the fetch starts after the first paint.
  ".wasm": "application/wasm" };
const which = process.argv[2];
const RUNS = Number(process.argv[3] || 5);

const TARGETS = {
  panel: {
    origin: "http://summrise.test",
    root: join(ROOT, "agent/resources"),
    index: () => readFileSync(process.env.P0_PANEL_HARNESS || "/tmp/p0/panel-harness.html"),
    entry: "/panel/", api: null,
  },
  console: {
    origin: "http://console.test",
    root: join(ROOT, "gateway/public"),
    index: () => readFileSync(join(ROOT, "gateway/public/index.html")),
    entry: "/", api: null,
  },
};
const t = TARGETS[which];
if (!t) { console.error("usage: first-render.mjs panel|console [runs]"); process.exit(2); }

const browser = await chromium.launch({ headless: true,
  executablePath: process.env.SUMMRISE_CHROMIUM_PATH || undefined,
  args: ["--no-sandbox", "--disable-dev-shm-usage"] });

const samples = [];
for (let i = 0; i < RUNS; i++) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await context.newPage();
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    if (url.origin !== t.origin) return route.abort();
    const p = url.pathname;
    if (p.startsWith("/api/"))
      return route.fulfill({ status: 401, contentType: "application/json",
        body: JSON.stringify({ type: "error", error: { message: "unauthorized" } }) });
    if (p === t.entry) return route.fulfill({ status: 200, contentType: "text/html", body: t.index() });
    const file = join(t.root, p);
    if (!existsSync(file)) return route.fulfill({ status: 404, body: "" });
    return route.fulfill({ status: 200, contentType: MIME[extname(file)] || "application/octet-stream",
      body: readFileSync(file) });
  });
  await page.goto(t.origin + t.entry, { waitUntil: "load" });
  await page.waitForTimeout(300);
  samples.push(await page.evaluate(() => {
    const paint = Object.fromEntries(performance.getEntriesByType("paint").map((e) => [e.name, e.startTime]));
    const nav = performance.getEntriesByType("navigation")[0] || {};
    const res = performance.getEntriesByType("resource");
    return { fcp: paint["first-contentful-paint"] ?? null, fp: paint["first-paint"] ?? null,
      domContentLoaded: nav.domContentLoadedEventEnd ?? null, load: nav.loadEventEnd ?? null,
      resources: res.length, decodedBytes: res.reduce((n, r) => n + (r.decodedBodySize || 0), 0),
      rootChildren: document.getElementById("root")?.children.length ?? -1,
      text: (document.getElementById("root")?.textContent || "").slice(0, 60) };
  }));
  await context.close();
}
await browser.close();

const f = samples.map((s) => s.fcp).filter((v) => v != null).sort((a, b) => a - b);
console.log(JSON.stringify({ which, runs: RUNS,
  fcp_ms_samples: samples.map((s) => s.fcp),
  fcp_ms_median: f.length ? f[Math.floor(f.length / 2)] : null,
  fcp_ms_min: f[0] ?? null, fcp_ms_max: f[f.length - 1] ?? null,
  first_paint_ms: samples.map((s) => s.fp),
  domContentLoaded_ms: samples.map((s) => s.domContentLoaded),
  load_ms: samples.map((s) => s.load),
  resources: samples[0].resources, decoded_bytes_sum: samples[0].decodedBytes,
  root_children: samples.map((s) => s.rootChildren), text_sample: samples[0].text }, null, 2));
