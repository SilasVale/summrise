#!/usr/bin/env node
// landing-contrast-check.mjs — the download landing page is a SURFACE, and nothing was measuring it.
//
// WHY (round 239). The token contract mentions the landing only because it shares three token names with the
// panel; the design suite covers the panel, the console and the extension. The landing is `index/src/page.js`,
// the page at the product's front door, hand-written with its own `--dsw-alias-*` vocabulary — and no check had
// ever asked whether its TEXT is readable.
//
// ITS VALUES ARE READ FROM THE FILE, NOT COPIED HERE. A check that hardcodes what it is checking tests itself.
// The pairs below are the ones the page actually paints: the three label weights on each of its three
// backgrounds, the button's foreground on its fill and hover, and the business state colour on a white card.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { contrastRatio, parseColour } from "../../agent/scripts/lib/contrast-probe.mjs";
import { PAGE as renderLanding } from "../../index/src/page.js";

const ROOT = fileURLToPath(new URL("../../", import.meta.url));
const PAGE = "index/src/page.js";

// BOTH BLOCKS, from the stylesheet the page actually renders. The light values are the ones outside the dark
// attribute; the dark ones are inside it, and a token the dark block does not restate keeps its light value.
const rendered = renderLanding("https://ai.saisi.online", "https://agent.saisi.online/vale-agent/vale-agent-latest.tgz", "vale setup");
const style = (/<style[\s\S]*?<\/style>/.exec(rendered) || [""])[0];
if (!style) throw new Error("the landing rendered no <style> — this check is reading the wrong thing");
const darkStart = style.indexOf("body[data-ds-dark-theme]");
if (darkStart < 0) throw new Error("no dark block in the landing's stylesheet — the theme toggle would do nothing");

const readVars = (css) => {
  const out = new Map();
  for (const m of css.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/(--[a-z0-9-]+)\s*:\s*(#[0-9a-fA-F]{3,8})\s*;/g)) {
    if (!out.has(m[1])) out.set(m[1], m[2]);
  }
  return out;
};
const LIGHT = readVars(style.slice(0, darkStart));
const DARK = new Map([...LIGHT, ...readVars(style.slice(darkStart))]);

const PAIRS = [
  ["label-primary on bg-base", "--dsw-alias-label-primary", "--dsw-alias-bg-base"],
  ["label-secondary on bg-base", "--dsw-alias-label-secondary", "--dsw-alias-bg-base"],
  ["label-tertiary on bg-base", "--dsw-alias-label-tertiary", "--dsw-alias-bg-base"],
  ["label-tertiary on layer-1", "--dsw-alias-label-tertiary", "--dsw-alias-bg-layer-1"],
  ["label-secondary on layer-2", "--dsw-alias-label-secondary", "--dsw-alias-bg-layer-2"],
  ["button foreground on its fill", "--dsw-alias-button-primary-foreground", "--dsw-alias-button-primary-fill"],
  ["button foreground on its hover", "--dsw-alias-button-primary-foreground", "--dsw-alias-button-primary-hover"],
  ["business state on a white card", "--dsw-alias-state-business-primary", "--dsw-alias-bg-layer-1"],
];

const AA_TEXT = 4.5;
const failures = [];
let checked = 0;
const themeVars = { light: LIGHT, dark: DARK };
for (const theme of ["light", "dark"]) {
  const vars = themeVars[theme];
  const v = (name) => {
    const value = vars.get(name);
    if (!value) throw new Error(`${PAGE} does not define ${name} for ${theme} — the check is reading the wrong thing`);
    return value;
  };
  for (const [label, fgName, bgName] of PAIRS) {
    const fg = v(fgName);
    const bg = v(bgName);
    const ratio = contrastRatio(parseColour(fg), parseColour(bg));
    checked++;
    if (ratio < AA_TEXT) {
      failures.push(`${theme}: ${label}: ${fg} on ${bg} measures ${ratio.toFixed(2)}, under the ${AA_TEXT} AA wants for text`);
    }
  }
}

// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN: a floor on the tokens found, so a rename in page.js fails loudly
// instead of silently checking nothing.
if (LIGHT.size < 10 || DARK.size < 10) {
  console.error(`landing contrast: FAILED — ${LIGHT.size} light / ${DARK.size} dark colours read from ${PAGE}, so this proves nothing`);
  process.exit(1);
}
if (failures.length) {
  console.error("landing contrast: FAILED");
  for (const f of failures) console.error("  " + f);
  process.exit(1);
}
console.log(`landing contrast: ok — ${PAIRS.length} pairs in BOTH themes (${checked} measurements), ${LIGHT.size} light / ${DARK.size} dark colours read from ${PAGE}`);

// ── the DOCUMENT's structure, which no sweep covers for this page ───────────────────────────────────────────
{
  const html = renderLanding("https://ai.saisi.online", "https://agent.saisi.online/vale-agent/vale-agent-latest.tgz", "vale setup");
  const body = html.replace(/<style[\s\S]*?<\/style>/g, "").replace(/<script[\s\S]*?<\/script>/g, "");
  const count = (re) => (body.match(re) || []).length;
  const h1s = [...body.matchAll(/<h1[^>]*>([^<]*)<\/h1>/g)].map((m) => m[1].trim());
  const title = (/<title>([^<]*)<\/title>/.exec(body) || [])[1];
  const problems = [];
  if (h1s.length !== 1) problems.push(`expected exactly 1 h1, found ${h1s.length}${h1s.length ? ": " + h1s.join(", ") : ""}`);
  if (title === undefined) problems.push("no <title>");
  if (h1s.length === 1 && title !== undefined && h1s[0] !== title) {
    problems.push(`the h1 says "${h1s[0]}" and the title says "${title}" — a page whose heading and title disagree announces two names`);
  }
  if (!/<html[^>]*lang=/.test(body)) problems.push("no lang on <html>");
  if (!/name="viewport"/.test(body)) problems.push("no viewport meta");
  if (count(/<main[ >]/g) !== 1) problems.push(`${count(/<main[ >]/g)} main landmarks, expected 1`);

  if (problems.length) {
    console.error("landing structure: FAILED");
    for (const p of problems) console.error("  " + p);
    process.exit(1);
  }
  console.log(`landing structure: ok — one h1 matching the title ("${h1s[0]}"), lang, viewport, one main`);
}
