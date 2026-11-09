// motion-gate — does the RENDERED console honour `prefers-reduced-motion`? (the fifty-second list)
//
// THE CONSOLE ALREADY DOES, AND THAT IS WHY THIS GATE EXISTS: it is the surface the panel was measured
// against, and until this round nothing checked the RESULT. Its blanket lives in `app.css`; its named
// lists silence the animations that must lose their state entirely. A stylesheet assertion covers the
// first and cannot cover the second — an animation added later with `animation: none` nowhere is exactly
// what a named list misses.
//
// THE MEASUREMENT IS THE DURATION, NOT THE PRESENCE. `transition-duration: 0.01ms` is the blanket
// switching motion OFF, so a presence check reports MORE motion after the rule than before it.
//
// Usage: node scripts/motion-gate.mjs        (serves the built console itself)
import { chromium } from "/home/zss/summrise/agent/resources/panel-react/node_modules/playwright-core/index.mjs";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";

const HTML = readFileSync(new URL("../dist/index.html", import.meta.url), "utf8");
const NOW = Date.now();

// A fixture small enough to read: one online machine, one offline, and a day with two client calls.
const ROUTES = {
  "/session": () => JSON.stringify({ authenticated: true, email: "gate@test", via: "token" }),
  "/health": () => JSON.stringify({ status: "ok", devices: 2, online: 1, uptime_s: 60, version: "gate" }),
  "/fleet": () => JSON.stringify([
    { id: "a", name: "DESKTOP-A", status: "online", version: "1", last_seen_ms: NOW, started_ms: NOW - 1000 },
    { id: "b", name: "DESKTOP-B", status: "offline", version: "1", last_seen_ms: NOW - 3600000, started_ms: NOW - 7200000 }
  ]),
  "/installer": () => JSON.stringify({ available: false }),
  "/audit/day": () => JSON.stringify({ client_calls: 2, failed: 0, console: 0, console_failed: 0, poll: 0,
    poll_failed: 0, unattributed: 0, day_start_ms: new Date().setHours(0, 0, 0, 0) })
};

const server = createServer((req, res) => {
  const path = new URL(req.url, "http://x").pathname;
  if (path === "/" || path === "/index.html") {
    res.writeHead(200, { "content-type": "text/html" });
    return res.end(HTML);
  }
  if (path === "/events") { res.writeHead(200, { "content-type": "text/event-stream" }); return res.end(""); }
  const body = ROUTES[path];
  res.writeHead(200, { "content-type": "application/json" });
  res.end(body ? body() : "[]");
});

/** Every element still MOVING for a readable length of time. 0.01ms is not motion. */
const moving = () => {
  const out = [];
  for (const el of document.querySelectorAll("body *")) {
    const s = getComputedStyle(el);
    for (const d of s.transitionDuration.split(",")) {
      const ms = parseFloat(d) * (d.includes("ms") ? 1 : 1000);
      if (ms > 1) { out.push(`${(el.className || el.tagName).toString().slice(0, 30)}:${d}`); break; }
    }
    if (s.animationName !== "none" && parseFloat(s.animationDuration) > 0.001) {
      out.push(`animation:${s.animationName} on ${(el.className || el.tagName).toString().slice(0, 26)}`);
    }
  }
  return out;
};

await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}`;

const b = await chromium.launch({ args: ["--no-sandbox"] });
try {
  // **A CONTEXT AND NOT A PAGE FLAG.** `reducedMotion` is a context option; emulating it afterwards
  // measures a page composed without it.
  const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
  const page = await ctx.newPage();
  await page.goto(base, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(1800);

  let total = 0;
  const pages = [
    { name: "Home", go: async () => {} },
    { name: "Activity", go: async () => page.locator('nav.rail button[title="Activity"]').click() },
    { name: "Device", go: async () => page.locator('nav.rail button[title="Device"]').click() }
  ];
  for (const p of pages) {
    await p.go();
    await page.waitForTimeout(p.name === "Home" ? 900 : 1600);
    const slow = await page.evaluate(moving);
    total += slow.length;
    if (slow.length) {
      throw new Error(`${p.name}: ${slow.length} element(s) still move under prefers-reduced-motion — ${slow.slice(0, 4).join(", ")}`);
    }
  }
  if (total !== 0) throw new Error(`reduced motion is not honoured: ${total} element(s) move`);
  console.log(`motion-gate: PASS — ${pages.length} pages, 0 elements moving under reduce`);
} finally {
  await b.close();
  server.close();
}
