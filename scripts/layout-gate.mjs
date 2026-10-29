// The layout gate: every page in a directory, at every width, must fit the window.
//
// Two callers: the console's review pages (rendered by `render_pages_for_review`) and the landing
// page in `landing/public`. It drives headless Chrome over the DevTools protocol to measure what a
// screenshot only shows to an eye — a card hiding a horizontal scrollbar, content painted past its
// clipping box, a page without one h1.
//
//   node scripts/layout-gate.mjs /tmp/console-shots/pages 390 800 1280
//   node scripts/layout-gate.mjs landing/public 390 768 1280
//
// Exits 1 on a failure, 2 when no Chromium is found.
import { existsSync, mkdirSync, readdirSync, realpathSync, writeFileSync } from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

// Absolute, always: `file://landing/public/index.html` is not a URL a browser can open, and Chrome
// answers it with its own error page — which passed every measurement until this line existed.
const pagesDir = resolve(process.argv[2] ?? "");
const widths = process.argv.slice(3).map(Number).filter(Boolean);
if (!process.argv[2] || widths.length === 0) {
  console.error("usage: node scripts/layout-gate.mjs <pages-dir> [width ...]");
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
    "--disable-dev-shm-usage",
    "--hide-scrollbars",
    "--remote-debugging-port=0",
    `--user-data-dir=${profile}`,
    "about:blank"
  ],
  { stdio: ["ignore", "ignore", "pipe"] }
);
let browserError = "";
browser.stderr.on("data", (chunk) => {
  browserError += chunk.toString();
  if (browserError.length > 4000) browserError = browserError.slice(-4000);
});

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function debugPort() {
  for (let attempt = 0; attempt < 150; attempt += 1) {
    try {
      const text = await import("node:fs").then((fs) =>
        fs.readFileSync(join(profile, "DevToolsActivePort"), "utf8")
      );
      return Number(text.split("\n")[0]);
    } catch {
      await sleep(100);
    }
  }
  console.error("the browser never opened its debugging port");
  console.error(`chrome: ${chrome} exit=${browser.exitCode ?? "running"}`);
  if (browserError.trim()) console.error(browserError.trim().slice(-1500));
  process.exit(2);
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
    if (over > 1) {
      const name =
        el.tagName.toLowerCase() +
        (el.id ? "#" + el.id : "") +
        (el.className ? "." + String(el.className).split(" ")[0] : "");
      panes.push(name + " +" + over);
    }
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
  // **ONE BLOCK MUST NOT SWALLOW THE PAGE.** Width overflow was the only thing measured, so a single
  // pre block 2883 px tall — 128 lines of JSON on the live System page — passed every gate while pushing
  // the rest of the page below the fold. Only blocks that are a unit of content are counted: a table
  // is tall because it has rows, a code block is tall because nobody bounded it.
  // **A BOX THAT DOES NOT HOLD ITS CONTENT.** The panel's action wrapped inside a fixed 56 px header
  // and printed on top of the paragraph below it: no width overflow, no clipping, nothing the width
  // rule could see. A container with overflow:visible whose flow children end below it is the
  // shape of that bug. Absolutely positioned children are overlays by design (menus, popups) and are
  // not counted.
  const spill = [];
  document.querySelectorAll("*").forEach((el) => {
    const cs = getComputedStyle(el);
    if (cs.display === "none" || cs.visibility === "hidden" || cs.overflow !== "visible") return;
    // **A CLOSED DISCLOSURE IS ONE VISIBLE CHILD, NOT MANY.** Chrome still answers with a box for
    // the body of a closed disclosure, so one whose body is taller than its summary
    // looked like a container painted past its own edge — reported on a menu nobody had opened.
    // Nothing in a closed disclosure is on screen; the rule is about what is.
    if (el.tagName === "DETAILS" && !el.open) return;
    const box = el.getBoundingClientRect();
    if (box.height <= 0) return;
    let bottom = 0;
    for (const child of el.children) {
      const style = getComputedStyle(child);
      if (style.position === "absolute" || style.position === "fixed" || style.display === "none") continue;
      const r = child.getBoundingClientRect();
      if (r.height > 0) bottom = Math.max(bottom, r.bottom);
    }
    if (bottom > box.bottom + 4) {
      const cls = String(el.className || "").trim().replace(/\s+/g, ".");
      spill.push(el.tagName.toLowerCase() + (cls ? "." + cls : "") + " by " + Math.round(bottom - box.bottom));
    }
  });
  const tall = [];
  document.querySelectorAll("pre, img, blockquote, .code-block").forEach((el) => {
    const r = el.getBoundingClientRect();
    if (r.height > innerHeight * 2) {
      const cls = String(el.className || "").trim().replace(/\\s+/g, ".");
      tall.push(el.tagName.toLowerCase() + (cls ? "." + cls : "") + " h=" + Math.round(r.height));
    }
  });
  return JSON.stringify({
    width: w,
    docOverflow,
    scrollWidth: doc.scrollWidth,
    h1: document.querySelectorAll("h1").length,
    overflow: docOverflow || panes.length > 0 || clipped.length > 0 || tall.length > 0 || spill.length > 0,
    panes: panes.slice(0, 4),
    clipped: clipped.slice(0, 4),
    tall: tall.slice(0, 4),
    spill: spill.slice(0, 4)
  });
})()`;

await send("Page.enable");
const shots = process.env.LAYOUT_SHOTS || "";
if (shots) mkdirSync(shots, { recursive: true });
if (!existsSync(pagesDir)) {
  console.error(`no such directory: ${pagesDir}`);
  process.exit(2);
}
const pages = readdirSync(pagesDir)
  .filter((name) => name.endsWith(".html"))
  .sort();
if (pages.length === 0) {
  console.error(`no pages in ${pagesDir}`);
  process.exit(2);
}
let failures = 0;
for (const width of widths) {
  for (const name of pages) {
    // The override is per document: setting it once per width let the first page of each width be
    // measured at the browser's own viewport.
    await send("Emulation.setDeviceMetricsOverride", {
      width,
      // The measurement viewport. A reviewer who wants a taller picture (the landing shot has to hold
      // three device rows) asks for one; the gate itself always measures the window it was given.
      height: Number(process.env.LAYOUT_VIEWPORT_HEIGHT || 900),
      deviceScaleFactor: 1,
      // `mobile: true` hands layout to the page's meta viewport, which is how a 1200px element in a
      // bare test page measured a 1208px viewport at width 390. The gate measures the CSS viewport
      // it asked for.
      mobile: false
    });
    const url = pathToFileURL(join(realpathSync(pagesDir), name)).href;
    const navigation = await send("Page.navigate", { url });
    // A navigation Chrome refused is not a page that fits: it is Chrome's own error document.
    if (navigation.result && navigation.result.errorText) {
      failures += 1;
      console.log(`FAIL ${name} @${width}: ${navigation.result.errorText} (${url})`);
      continue;
    }
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
    if (shots) {
      const picture = await send("Page.captureScreenshot", { format: "png" });
      writeFileSync(
        join(shots, `${name.replace(/\.html$/, "")}-${width}.png`),
        Buffer.from(picture.result.data, "base64")
      );
    }
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
      ? [
          result.docOverflow ? `overflow ${result.scrollWidth}>${result.width}` : "",
          (result.panes || []).length ? `panes=${result.panes.join(",")}` : "",
          (result.clipped || []).length ? `clipped=${result.clipped.join(",")}` : "",
          (result.tall || []).length ? `tall=${result.tall.join(",")}` : "",
          (result.spill || []).length ? `spill=${result.spill.join(",")}` : ""
        ]
          .filter(Boolean)
          .join(" ")
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
