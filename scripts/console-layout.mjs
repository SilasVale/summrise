// The console's layout gate: every rendered page, at every width, must fit the window.
//
// The review renderer writes the pages; this drives headless Chrome over the DevTools protocol to
// measure what the screenshots only show to an eye. A page wider than the window is the failure
// this catches — it has been found three times by looking, and looking is not a gate.
//
//   node scripts/console-layout.mjs /tmp/console-shots/pages 390 800 1280
//
// Exits 1 when a page overflows or does not have exactly one <h1>.
import { readdirSync } from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const pagesDir = process.argv[2];
const widths = process.argv.slice(3).map(Number).filter(Boolean);
if (!pagesDir || widths.length === 0) {
  console.error("usage: node scripts/console-layout.mjs <pages-dir> [width ...]");
  process.exit(2);
}

// The browser is whichever Chromium this machine has; CI's runner ships `google-chrome` and a
// developer's box may ship `chromium`, so the gate asks instead of assuming.
const which = (name) => {
  const attempt = spawnSync(name, ["--version"], { stdio: "ignore" });
  return attempt.status === 0 ? name : null;
};
const chrome = [
  process.env.CHROME,
  "google-chrome",
  "google-chrome-stable",
  "chromium",
  "chromium-browser"
]
  .filter(Boolean)
  .map(which)
  .find(Boolean);
if (!chrome) {
  console.error("no Chromium found (set CHROME=/path/to/chrome)");
  process.exit(2);
}
const profile = mkdtempSync(join(tmpdir(), "console-layout-"));
const browser = spawn(
  chrome,
  [
    "--headless=new",
    "--disable-gpu",
    "--no-sandbox",
    "--hide-scrollbars",
    "--remote-debugging-port=0",
    `--user-data-dir=${profile}`,
    "about:blank"
  ],
  { stdio: "ignore" }
);

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function debugPort() {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      const text = await import("node:fs").then((fs) =>
        fs.readFileSync(join(profile, "DevToolsActivePort"), "utf8")
      );
      return Number(text.split("\n")[0]);
    } catch {
      await sleep(100);
    }
  }
  throw new Error("the browser never opened its debugging port");
}

const port = await debugPort();
const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
const page = targets.find((target) => target.type === "page");
const socket = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.onopen = resolve;
  socket.onerror = reject;
});

let next = 0;
const pending = new Map();
socket.onmessage = (event) => {
  const message = JSON.parse(event.data);
  if (message.id && pending.has(message.id)) {
    pending.get(message.id)(message);
    pending.delete(message.id);
  }
};
const send = (method, params = {}) =>
  new Promise((resolve) => {
    const id = (next += 1);
    pending.set(id, resolve);
    socket.send(JSON.stringify({ id, method, params }));
  });

const probe = `(() => {
  const w = window.innerWidth;
  const doc = document.documentElement;
  // The shell gives the content pane its own scroll box, so a too-wide card does not widen the
  // document — it raises a horizontal scrollbar inside the pane. Both are the same failure and both
  // are measured.
  const panes = [];
  document.querySelectorAll("*").forEach((el) => {
    const cs = getComputedStyle(el);
    if (cs.display === "none" || cs.visibility === "hidden") return;
    if (cs.overflowX !== "auto" && cs.overflowX !== "scroll") return;
    const over = el.scrollWidth - el.clientWidth;
    if (over > 1) panes.push((el.id ? "#" + el.id : el.tagName.toLowerCase()) + " +" + over);
  });
  // An element that neither the window nor any scroll box can reach is clipped content.
  const clipped = [];
  const hiddenBox = (node) => {
    for (let a = node; a; a = a.parentElement) {
      const cs = getComputedStyle(a);
      if (cs.clip && cs.clip.startsWith("rect(0px, 0px, 0px, 0px)")) return true;
    }
    return false;
  };
  const edge = (el) => {
    for (let a = el.parentElement; a; a = a.parentElement) {
      const cs = getComputedStyle(a);
      if (hiddenBox(a)) continue;
      const o = cs.overflowX;
      if (o === "auto" || o === "scroll") return null;
      if (o === "hidden" || o === "clip") return a.getBoundingClientRect().right;
    }
    return w;
  };
  document.querySelectorAll("*").forEach((el) => {
    const cs = getComputedStyle(el);
    if (cs.display === "none" || cs.visibility === "hidden" || cs.position === "fixed") return;
    if (hiddenBox(el)) return;
    const r = el.getBoundingClientRect();
    if (r.width <= 0) return;
    const limit = edge(el);
    if (limit !== null && r.right > limit + 1) {
      clipped.push(el.tagName.toLowerCase() + (el.id ? "#" + el.id : "") + " right=" + Math.round(r.right) + " limit=" + Math.round(limit));
    }
  });
  const docOverflow = doc.scrollWidth > w + 1;
  return JSON.stringify({
    width: w,
    docOverflow,
    scrollWidth: doc.scrollWidth,
    h1: document.querySelectorAll("h1").length,
    overflow: docOverflow || panes.length > 0 || clipped.length > 0,
    panes: panes.slice(0, 4),
    clipped: clipped.slice(0, 4)
  });
})()`;

await send("Page.enable");
const pages = readdirSync(pagesDir)
  .filter((name) => name.endsWith(".html"))
  .sort();
let failures = 0;
for (const width of widths) {
  for (const name of pages) {
    // The override is per document: setting it once per width let the first page of each width be
    // measured at the browser's own viewport.
    await send("Emulation.setDeviceMetricsOverride", {
      width,
      height: 900,
      deviceScaleFactor: 1,
      // `mobile: true` hands layout to the page's meta viewport, which is how a 1200px element in a
      // bare test page measured a 1208px viewport at width 390. The gate measures the CSS viewport
      // it asked for.
      mobile: false
    });
    await send("Page.navigate", { url: `file://${join(pagesDir, name)}` });
    // The evaluation must land in the new document, after layout: a fixed sleep raced the load and
    // measured the previous page's viewport.
    for (let attempt = 0; attempt < 40; attempt += 1) {
      const ready = await send("Runtime.evaluate", {
        expression: "document.readyState",
        returnByValue: true
      });
      if (ready.result.result.value === "complete") break;
      await sleep(50);
    }
    await send("Runtime.evaluate", {
      expression: "new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))",
      awaitPromise: true
    });
    const answer = await send("Runtime.evaluate", { expression: probe, returnByValue: true });
    const result = JSON.parse(answer.result.result.value);
    const ok = !result.overflow && result.h1 === 1;
    if (!ok) failures += 1;
    if (process.env.LAYOUT_EVAL && name === process.env.LAYOUT_PAGE) {
      const deep = await send("Runtime.evaluate", {
        expression: process.env.LAYOUT_EVAL,
        returnByValue: true
      });
      console.log(name, width, JSON.stringify(deep.result.result.value));
    }
    if (process.env.LAYOUT_DEBUG) console.log(name, width, answer.result.result.value);
    const note = result.overflow
      ? `overflow ${result.scrollWidth}>${result.width} panes=${(result.panes || []).join(",")} clipped=${(result.clipped || []).join(",")}`
      : result.h1 !== 1
        ? `h1=${result.h1}`
        : "";
    if (!ok) console.log(`FAIL ${name} @${width}: ${note}`);
  }
}
socket.close();
browser.kill();
try {
  rmSync(profile, { recursive: true, force: true });
} catch {
  // A profile Chrome still holds is the operating system's to clean up; the gate has answered.
}
console.log(`${pages.length} pages x ${widths.join("/")} -> ${failures} failure(s)`);
process.exit(failures === 0 ? 0 : 1);
