#!/usr/bin/env node
// Summrise landing page — the checks that keep two languages and one page honest.
//
// No dependencies: this is the whole gate. It runs on a checkout with no npm install, which is
// the point — a page nobody can check is a page that drifts.
//
//   node check.mjs
//
// What it proves:
//   1. every data-i18n key in the markup has a Chinese string, and every Chinese string is used
//      (an unused translation is a string nobody can see; a missing one is English leaking out);
//   2. ids are unique and every in-page anchor resolves;
//   3. local assets referenced by the page exist;
//   4. the page has its one language switch, its title and its description;
//   5. no placeholder text survived into the deliverable.
import { readFileSync, existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const root = dirname(fileURLToPath(import.meta.url));
const publicDir = resolve(root, "public");
const html = readFileSync(resolve(publicDir, "index.html"), "utf8");

const failures = [];
const fail = (msg) => failures.push(msg);

/* ---------- 1. translations ---------- */

const sandbox = { window: {} };
vm.runInNewContext(readFileSync(resolve(publicDir, "assets/lang-zh.js"), "utf8"), sandbox);
const zh = sandbox.window.SUMMRISE_ZH;
if (!zh || typeof zh !== "object") fail("assets/lang-zh.js did not define window.SUMMRISE_ZH");

const used = new Set();
for (const m of html.matchAll(/data-i18n="([^"]+)"/g)) {
  if (used.has(m[1])) fail(`data-i18n key used twice: ${m[1]}`);
  used.add(m[1]);
}
// a string may be used from the script as well as from the markup (the release fallback)
const script = readFileSync(resolve(publicDir, "assets/site.js"), "utf8");
for (const m of script.matchAll(/ZH\["([^"]+)"\]/g)) used.add(m[1]);

for (const key of used) if (!(key in zh)) fail(`missing Chinese string: ${key}`);
for (const key of Object.keys(zh)) if (!used.has(key)) fail(`unused Chinese string: ${key}`);

/* ---------- 2. ids and anchors ---------- */

const ids = new Set();
for (const m of html.matchAll(/\sid="([^"]+)"/g)) {
  if (ids.has(m[1])) fail(`duplicate id: ${m[1]}`);
  ids.add(m[1]);
}
for (const m of html.matchAll(/href="#([^"]+)"/g)) {
  if (!ids.has(m[1])) fail(`anchor points at no id: #${m[1]}`);
}

/* ---------- 3. local assets ---------- */

for (const m of html.matchAll(/(?:src|href)="(?!https?:|#|mailto:)([^"]+)"/g)) {
  const path = resolve(publicDir, m[1]);
  if (!existsSync(path)) fail(`local asset missing: ${m[1]}`);
}

/* ---------- 4. the page's own furniture ---------- */

if (!/<html lang="en"/.test(html)) fail('the page must default to lang="en"');
if (!html.includes('id="lang-toggle"')) fail("no language switch (#lang-toggle)");
if (!/<title>[^<]{10,}<\/title>/.test(html)) fail("missing or too-short <title>");
if (!/<meta name="description" content="[^"]{40,}"/.test(html)) fail("missing or too-short meta description");
if (!/<script src="assets\/lang-zh\.js"><\/script>/.test(html)) fail("lang-zh.js is not loaded");
if (!/<script src="assets\/site\.js"><\/script>/.test(html)) fail("site.js is not loaded");
if (!html.includes('class="skip"')) fail("no skip link");

/* ---------- 5. no placeholders ---------- */

for (const bad of ["Lorem ipsum", "TODO", "FIXME", "xxx", "PLACEHOLDER"]) {
  if (html.includes(bad)) fail(`placeholder text in the page: ${bad}`);
}

/* ---------- 6. contrast, measured the way the console measures it ---------- */

const css = readFileSync(resolve(publicDir, "assets/site.css"), "utf8");
const rootBlock = (css.match(/:root\s*\{([\s\S]*?)\}/) || [])[1] || "";
const vars = {};
for (const m of rootBlock.matchAll(/--([a-z0-9-]+):\s*(#[0-9a-fA-F]{3,8})\s*;/g)) vars[m[1]] = m[2];

function channel(v) {
  v /= 255;
  return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
}
function luminance(hex) {
  const h = hex.replace("#", "");
  const full = h.length === 3 ? h.split("").map((c) => c + c).join("") : h;
  const r = parseInt(full.slice(0, 2), 16), g = parseInt(full.slice(2, 4), 16), b = parseInt(full.slice(4, 6), 16);
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}
function contrast(a, b) {
  const la = luminance(a), lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}
const hex = (name) => (name.startsWith("#") ? name : vars[name]);

// Every pair below is one the page actually renders as text. 4.5 is WCAG AA for normal text;
// meta lines here are 9.5-12.5px, so there is no large-text exemption to take.
const pairs = [
  ["ink", "paper", "body copy"],
  ["muted", "paper", "secondary copy"],
  ["faint", "paper", "meta, kickers, captions"],
  ["accent", "paper", "links and the eyebrow"],
  ["accent", "accent-soft", "the soft accent chip"],
  ["ok", "#e7f6ec", "the ok pill"],
  ["warn", "#fff4e5", "the warn pill"],
  ["#cfcdc6", "#0b0d0f", "terminal body"],
  ["#8a8c92", "#0b0d0f", "terminal dim"],
  ["#8f9197", "#0b0d0f", "terminal comments"],
  ["#f2efe9", "dark", "band copy"],
  ["#b6b3ab", "dark", "band body"],
  ["#8f9198", "#141519", "band fact labels"],
  ["#ffa36b", "dark", "band kicker"],
];
let measured = 0;
for (const [fgName, bgName, where] of pairs) {
  const fg = hex(fgName), bg = hex(bgName);
  if (!fg || !bg) { fail(`contrast pair names a missing token: ${fgName} on ${bgName}`); continue; }
  const r = contrast(fg, bg);
  measured++;
  if (r < 4.5) fail(`contrast ${r.toFixed(2)}:1 < 4.5 for ${where} (${fg} on ${bg})`);
}

/* ---------- 7. the social card ---------- */

for (const prop of ["og:title", "og:description", "og:url", "og:image"]) {
  if (!html.includes(`property="${prop}"`)) fail(`missing ${prop}`);
}
const ogPath = resolve(publicDir, "assets/og.png");
if (!existsSync(ogPath)) {
  fail("assets/og.png is missing (run ./render-og.sh)");
} else {
  const png = readFileSync(ogPath);
  const isPng = png.length > 24 && png.toString("ascii", 1, 4) === "PNG";
  const w = png.readUInt32BE(16), h = png.readUInt32BE(20);
  if (!isPng) fail("assets/og.png is not a PNG");
  else if (w !== 1200 || h !== 630) fail(`assets/og.png is ${w}x${h}, expected 1200x630`);
}

/* ---------- report ---------- */

if (failures.length) {
  console.error(`landing: ${failures.length} problem(s)`);
  for (const f of failures) console.error("  FAIL " + f);
  process.exit(1);
}
console.log(`landing: ok — ${used.size} strings in two languages, ${ids.size} ids, all anchors and assets resolve, ${measured} contrast pairs >= 4.5:1`);
