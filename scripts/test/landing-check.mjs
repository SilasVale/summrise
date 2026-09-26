#!/usr/bin/env node
// ── THE MUTATION THAT MUST FAIL THIS GATE (moved here from the ledger table, landing 4b) ──
// Read this when you change this file: the mutation is how you find out whether the gate can still
// fail at all. A gate that cannot be broken is worse than no gate.
//
// MUTATION: lighten a label (the tertiary `#71717a` → `#9a9aa2`), take the heading away (the `<h1>` back to a `<div>`), or give a card a fixed width (`.card { width: 480px }`)
// RESULT:   exit 1 for each: "2.68 on #fafafa, under the 4.5 AA wants for text" · "expected exactly 1 h1, found 0" · ".card { width: 480px } — wider than the 320px a 1.4.10 reflow test uses". It reads page.js's own values and BOTH themes (renamed from landing-contrast-check in round 242: a name covering a third of what it does is the kind that stops the next reader looking)

// landing-check.mjs — the download landing page is a SURFACE, and nothing was measuring it.
//
// RENAMED FROM landing-contrast-check.mjs (round 242): it checks the page's COLOURS, its DOCUMENT structure and
// its LAYOUT now, and a file named for one third of what it does is the kind of name that stops the next reader
// looking — the same lesson as the console's "unified brand tokens" comment (233) and the violet that existed
// nowhere (234).
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
const rendered = renderLanding("https://ai.saisi.online", "https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz", "summrise setup");
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
  const html = renderLanding("https://ai.saisi.online", "https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz", "summrise setup");
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

// ── the LAYOUT's structural invariants, which need no browser ───────────────────────────────────────────────
// Round 224 found two grids in the console whose FIXED pixel minimums (330px, 260px) overflowed their containers
// at 320 — the width WCAG 1.4.10 names. This page is checked the same way, statically: no rule may declare a
// width or min-width wider than the narrowest viewport the page has to survive, and there must be at least one
// breakpoint, or nothing about it can reflow at all.
{
  const rendered = renderLanding("https://ai.saisi.online", "https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz", "summrise setup");
  const style = (/<style[\s\S]*?<\/style>/.exec(rendered) || [""])[0];
  const NARROWEST = 320;
  const problems = [];
  let widths = 0;
  for (const m of style.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const sel = m[1].trim().split("\n").pop().trim();
    for (const decl of m[2].matchAll(/(?:^|;)\s*(min-width|width)\s*:\s*([^;]+)/g)) {
      const value = decl[2].trim();
      const px = /^(\d+(?:\.\d+)?)px$/.exec(value);
      if (!px) continue;                       // percentages, clamp(), min() and calc() all shrink
      widths++;
      if (Number(px[1]) > NARROWEST) {
        problems.push(`${sel} { ${decl[1]}: ${value} } — wider than the ${NARROWEST}px a 1.4.10 reflow test uses; a max-width would cap it instead of setting a floor`);
      }
    }
  }
  const breakpoints = (style.match(/@media[^{]*max-width[^{]*/g) || []).length;
  if (breakpoints === 0) problems.push("no max-width breakpoint at all, so nothing about this page can reflow");
  if (widths === 0) problems.push("no pixel width was found to check — the scan is reading the wrong thing");

  if (problems.length) {
    console.error("landing layout: FAILED");
    for (const p of problems) console.error("  " + p);
    process.exit(1);
  }
  console.log(`landing layout: ok — ${widths} fixed width(s), none above ${NARROWEST}px, ${breakpoints} breakpoint(s)`);
}

// ── the update command must be installable ─────────────────────────────────────────────────────────
// AGENTS.md: "A BARE \`npm i -g summrise-agent\` CAN INSTALL NOTHING WHILE REPORTING SUCCESS" — a stale
// cached \`latest\` prints "changed 1 package" and leaves the old CLI in place. The landing's step 3 told
// every visitor to run exactly that (round 128, from the twelfth exploration) while step 2 and both
// READMEs used the URL form — and the paragraph even ended "not with npm's exit code", so the class was
// known and the safe form was one line away.
{
  const src = readFileSync(PAGE, "utf8");
  // A bare package name is one with no URL and no --prefix in front of it.
  const bare = [...src.matchAll(/npm i -g (?:--prefix [^<]*)? ?summrise-agent\b/g)];
  if (bare.length) {
    console.error("landing install: FAILED");
    for (const b of bare) console.error("  a bare package name has a resolution step: " + b[0]);
    process.exit(1);
  }
  // A FLOOR, like this file's other scans: reading no command at all is not a clean scan. A rename in
  // page.js must fail loudly rather than turn this into a check of nothing.
  const seen = (src.match(/npm i -g/g) || []).length;
  if (seen < 2) {
    console.error(`landing install: FAILED — read ${seen} "npm i -g" occurrence(s); the scan is reading the wrong thing`);
    process.exit(1);
  }
  console.log(`landing install: ok — ${seen} install command(s), none naming the bare package`);
}
