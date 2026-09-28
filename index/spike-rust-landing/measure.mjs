// measure.mjs — ONE evaluate, used on the live page and on both local arms, in one
// browser session, at 1280x900. "It felt fast" is not a number; this is.
//
// Three targets, and the third is the one that decides:
//   live     https://agent.saisi.online/         — the page as the world gets it (over the network)
//   pagejs   dist/pagejs-reference.html          — page.js output, served from THIS box
//   rust     dist/index.html                     — the Rust page, served from THIS box
//
// `pagejs` vs `rust` is the honest comparison: same server, same session, same viewport,
// so the only difference is the page. `live` is the context the brief asked for.
//
// Usage: node measure.mjs        (needs playwright-core; see README)
import { createServer } from "node:http";
import { readFileSync, existsSync } from "node:fs";
import { extname, join } from "node:path";
import { chromium } from "playwright-core";

const TYPES = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".wasm": "application/wasm" };
const server = createServer((req, res) => {
  const p = join("dist", decodeURIComponent(new URL(req.url, "http://x").pathname).replace(/^\/+/, "") || "index.html");
  if (!existsSync(p)) { res.writeHead(404).end("not found"); return; }
  res.writeHead(200, { "content-type": TYPES[extname(p)] || "application/octet-stream", "cache-control": "no-store" });
  res.end(readFileSync(p));
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));   // loopback only
const PORT = server.address().port;

const EVALUATE = () => {
  const vis = (el) => {
    const s = getComputedStyle(el);
    if (s.display === "none" || s.visibility === "hidden" || Number(s.opacity) === 0) return false;
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  const lum = ([r, g, b]) => {
    const f = (c) => { c /= 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  };
  const parse = (s) => (s.match(/[\d.]+/g) || []).map(Number);
  const bgOf = (el) => {
    for (let n = el; n; n = n.parentElement) {
      const c = parse(getComputedStyle(n).backgroundColor);
      if (c.length >= 3 && (c.length < 4 || c[3] > 0.9)) return c.slice(0, 3);
    }
    return [255, 255, 255];
  };
  const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };

  const texts = [];
  const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n = walk.nextNode(); n; n = walk.nextNode()) {
    if (!n.textContent.trim()) continue;
    const el = n.parentElement;
    if (!el || !vis(el)) continue;
    texts.push(el);
  }
  const rows = texts.map((el) => {
    const s = getComputedStyle(el);
    const fg = parse(s.color).slice(0, 3), bg = bgOf(el);
    const size = parseFloat(s.fontSize), weight = Number(s.fontWeight) || 400;
    const large = size >= 24 || (size >= 18.66 && weight >= 700);
    return { size, weight, ratio: +ratio(fg, bg).toFixed(2), need: large ? 3 : 4.5, text: el.textContent.trim().slice(0, 40) };
  });
  const failing = rows.filter((r) => r.ratio < r.need);

  const ladder = [...new Set(rows.map((r) => r.size))].sort((a, b) => b - a);
  const h1 = document.querySelector("h1");
  const btn = document.querySelector(".btn-primary");
  const costEl = [...document.querySelectorAll("body *")].find(
    (e) => e.children.length === 0 && /Easiest path: one setup\.exe/.test(e.textContent));
  const br = btn && btn.getBoundingClientRect();
  const bs = btn && getComputedStyle(btn);

  const nav = performance.getEntriesByType("navigation")[0] || {};
  const fcp = performance.getEntriesByName("first-contentful-paint")[0];
  return {
    textNodes: rows.length,
    contrastFailures: failing.length,
    worstContrast: rows.length ? Math.min(...rows.map((r) => r.ratio)) : null,
    ladder,
    h1: h1 ? { size: parseFloat(getComputedStyle(h1).fontSize), weight: getComputedStyle(h1).fontWeight } : null,
    h2Count: document.querySelectorAll("h2").length,
    costBeforeClick: !!(costEl && br && costEl.getBoundingClientRect().top < br.top),
    button: btn ? {
      background: bs.backgroundColor, radius: bs.borderRadius,
      w: Math.round(br.width), h: Math.round(br.height), color: bs.color,
    } : null,
    hasConsoleLink: !!document.querySelector('.desc a[href]'),
    hasFallbackChannel: document.body.innerHTML.includes("npm i -g"),
    canvasMotes: !!document.querySelector("canvas[aria-hidden=true]"),
    // text-node counting, three definitions, so a count that disagrees with
    // somebody else's number can be traced to the definition rather than argued about
    textNodesAll: (() => { let n = 0; const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
      for (let t = w.nextNode(); t; t = w.nextNode()) if (t.textContent.trim()) n++; return n; })(),
    textNodesRendered: (() => { let n = 0; const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
      for (let t = w.nextNode(); t; t = w.nextNode()) {
        if (!t.textContent.trim()) continue;
        const el = t.parentElement; if (!el || /^(SCRIPT|STYLE|NOSCRIPT)$/.test(el.tagName) || !vis(el)) continue; n++; } return n; })(),
    wasmReadyMs: window.__wasmReadyMs ?? null,
    fcpMs: fcp ? +fcp.startTime.toFixed(1) : null,
    lcpMs: window.__lcp ? +window.__lcp.toFixed(1) : null,
    domContentLoadedMs: nav.domContentLoadedEventEnd ? +nav.domContentLoadedEventEnd.toFixed(1) : null,
    loadMs: nav.loadEventEnd ? +nav.loadEventEnd.toFixed(1) : null,
  };
};

const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.SUMMRISE_CHROMIUM_PATH || undefined,
  args: ["--no-sandbox", "--disable-dev-shm-usage"],
});
const results = {};
for (const [name, url] of [
  ["live", "https://agent.saisi.online/"],
  ["pagejs", `http://127.0.0.1:${PORT}/pagejs-reference.html`],
  ["rust", `http://127.0.0.1:${PORT}/index.html`],
  // THE DECISIVE ARM. The brief's hard case is "a landing page must render BEFORE a
  // binary downloads". Loopback is too fast to show it, so this arm holds the wasm
  // back for 2s. If FCP is unmoved while wasmReadyMs jumps to ~2s, the design is
  // proven rather than asserted.
  ["rust-slowwasm", `http://127.0.0.1:${PORT}/index.html`],
]) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  if (name === "rust-slowwasm") {
    await page.route("**/*.wasm", async (route) => {
      await new Promise((r) => setTimeout(r, 2000));
      await route.continue();
    });
  }
  await page.addInitScript(() => {
    window.__lcp = 0;
    try {
      new PerformanceObserver((l) => { const e = l.getEntries(); window.__lcp = e[e.length - 1].startTime; })
        .observe({ type: "largest-contentful-paint", buffered: true });
    } catch {}
  });
  await page.goto(url, { waitUntil: "load" });
  await page.waitForTimeout(2500);            // let wasm boot and the field start
  results[name] = await page.evaluate(EVALUATE);
  // DOES THE BEHAVIOUR ACTUALLY WORK? A canvas in the DOM proves the wasm ran;
  // clicking the toggle proves the listener is attached. Claiming either without
  // this would be exactly the kind of unverified sentence this repo forbids.
  results[name].toggleWorks = await page.evaluate(async () => {
    const b = document.querySelector(".theme-toggle");
    if (!b) return "no button";
    const before = document.body.hasAttribute("data-ds-dark-theme");
    b.click();
    await new Promise((r) => setTimeout(r, 60));
    const after = document.body.hasAttribute("data-ds-dark-theme");
    const stored = localStorage.getItem("summrise-theme");
    b.click();                                   // put it back
    await new Promise((r) => setTimeout(r, 60));
    return before !== after && (stored === "dark" || stored === "light") ? "ok" : `no change (${before}->${after}, stored=${stored})`;
  });
  results[name].canvasPainted = await page.evaluate(() => {
    const c = document.querySelector("canvas[aria-hidden=true]");
    if (!c) return "no canvas";
    const ctx = c.getContext("2d");
    const d = ctx.getImageData(0, 0, c.width, c.height).data;
    let lit = 0;
    for (let i = 3; i < d.length; i += 4) if (d[i] > 0) lit++;
    return lit > 0 ? `ok (${lit} painted px)` : "canvas is blank";
  });
  results[name].bytesServed = (await page.content()).length;
  await page.close();
}
await browser.close();
server.close();

const KEYS = ["textNodes","textNodesAll","textNodesRendered","contrastFailures","worstContrast","ladder","h1","h2Count","costBeforeClick","button","hasConsoleLink","hasFallbackChannel","canvasMotes","toggleWorks","canvasPainted","fcpMs","lcpMs","wasmReadyMs","domContentLoadedMs","loadMs"];
console.log("\n=== the same evaluate, three targets, 1280x900, one session ===");
for (const k of KEYS) {
  console.log(`  ${k.padEnd(18)} live=${JSON.stringify(results.live[k])}`);
  console.log(`  ${"".padEnd(18)} pagejs=${JSON.stringify(results.pagejs[k])}  rust=${JSON.stringify(results.rust[k])}  rust-slowwasm=${JSON.stringify(results["rust-slowwasm"][k])}`);
}
console.log("\nVERDICT shape identical (live vs rust):",
  JSON.stringify(results.live.ladder) === JSON.stringify(results.rust.ladder) &&
  results.live.worstContrast === results.rust.worstContrast &&
  results.live.costBeforeClick === results.rust.costBeforeClick &&
  JSON.stringify(results.live.button) === JSON.stringify(results.rust.button));
