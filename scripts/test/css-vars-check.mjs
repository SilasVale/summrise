// EVERY `var(--token)` MUST RESOLVE. An undefined custom property does not fall back to something
// sensible — it makes the WHOLE DECLARATION invalid at computed-value time, silently. That is how the
// console's keyboard focus ring came to be invisible on every control (round 137): a sheet styled
// `outline: 2px solid var(--focus-ring)` and `box-shadow: 0 0 0 3px var(--focus-ring-soft)`, neither token
// was defined in that sheet, and BOTH declarations were dropped. The CSS read as correct. The page looked
// fine. Only a rendered measurement of the focus ring found it, and only because rounds 133-136 had just
// repaired the instrument that could see it.
//
// A missing definition is not a thing to find by rendering. It is a string that has no counterpart, which
// is exactly what a check is good at — so this gate makes the class impossible to reintroduce.
//
// SCOPE: each UI is scanned as a WHOLE, because token definitions and their uses live in different files
// (the panel's tokens.css defines what components.css and desktop.css consume). Per-file scanning would
// report thousands of false positives.
//
// NOT A FALSE POSITIVE: a `var(--x, fallback)` needs no definition — the fallback is the answer. Those are
// skipped deliberately rather than counted, and the count of skipped ones is printed so a stylesheet that
// suddenly becomes all-fallbacks is visible.
import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..", "..");

const UIS = {
  panel: ["agent/resources/panel-react/src/styles"],
  console: ["gateway/ui/src/styles"],
};

let failures = 0;
const ok = (name, cond, detail = "") => {
  if (cond) console.log(`ok: ${name}`);
  else {
    failures++;
    console.log(`FAIL: ${name}${detail ? ` — ${detail}` : ""}`);
  }
};

/** Every .css file under the given directories, without descending into build output. */
function cssFiles(dirs) {
  const out = [];
  for (const dir of dirs) {
    const full = path.join(ROOT, dir);
    if (!existsSync(full)) continue;
    const walk = (d) => {
      for (const name of readdirSync(d)) {
        if (name === "node_modules" || name === "dist" || name === "build") continue;
        const f = path.join(d, name);
        if (statSync(f).isDirectory()) walk(f);
        else if (f.endsWith(".css")) out.push(f);
      }
    };
    walk(full);
  }
  return [...new Set(out)];
}

// ── AND THE OTHER DIRECTION: A TOKEN NOTHING READS (round 20 of the standing goal) ─────────────────────────
// The check above finds a reference with no definition. Nothing found a DEFINITION with no reference, and there
// were twenty-one of them across the panel and the console — dead weight in the one block a reader goes to in
// order to learn what the palette IS. The rule is the CSS-rules ratchet's: a token may be read by a RULE, by
// CODE (`getPropertyValue`), or by a name ASSEMBLED at runtime, and anything else is a name to delete.
//
// THE TWO ENTRIES BELOW ARE THE ONES A NAIVE SCAN GETS WRONG, which is why they are named with their reasons
// rather than filtered by a pattern:
const UNREAD_OK = new Map([
  ["--ds-neutral-", "a FAMILY, read assembled: themeContrast.test.ts builds `--ds-neutral-${step}` from a step number — the same false positive the dead-CSS tool documents for class names"],
  ["--glass-blur", "declared on BOTH UIs because the token contract REQUIRES it present for the landing comparison (the art-direction palette must be covered); the landing is the surface that reads it"],
]);

let totalDefs = 0;
let totalRefs = 0;
let totalUnread = 0;
for (const [ui, dirs] of Object.entries(UIS)) {
  const files = cssFiles(dirs);
  ok(`${ui}: stylesheets found`, files.length > 0, "no .css under " + dirs.join(", "));
  // COMMENTS ARE NOT DECLARATIONS. The `defined` set was built from raw text, so a comment that MENTIONS a token
  // (`/* --ds-dur: 0s does not reach an animation ─ */`, `/* --warn-ink, not --amber-bright, because… */`) counted
  // as a declaration of it — and the new unread-token check then reported two tokens that exist only as prose in
  // an explanation. `retired-colours-check` carries the identical lesson from its own first run, which is why it
  // strips comments first; this gate now does too. It also makes the reference check honest: a `var()` inside a
  // comment is not a reference either.
  const text = files.map((f) => readFileSync(f, "utf8")).join("\n").replace(/\/\*[\s\S]*?\*\//g, "");
  const defined = new Set([...text.matchAll(/(--[a-z0-9-]+)\s*:/g)].map((m) => m[1]));
  // A reference WITH a fallback resolves; one without must have a definition.
  const bare = new Set();
  let withFallback = 0;
  for (const m of text.matchAll(/var\(\s*(--[a-z0-9-]+)\s*([,)])/g)) {
    if (m[2] === ",") withFallback++;
    else bare.add(m[1]);
  }
  totalDefs += defined.size;
  totalRefs += bare.size;
  const missing = [...bare].filter((v) => !defined.has(v)).sort();
  ok(
    `${ui}: every var() reference resolves`,
    missing.length === 0,
    missing.length
      ? `${missing.length} undefined — each one silently drops its whole declaration:\n    ${missing.join("\n    ")}`
      : "",
  );
  console.log(`    ${defined.size} defined · ${bare.size} referenced without fallback · ${withFallback} with a fallback`);

  // Every token is read by a rule in this sheet, by code in the app, or by one of the two documented names.
  const codeDirs = ui === "panel" ? ["agent/resources/panel-react/src"] : ["gateway/ui/src"];
  const codeFiles = [];
  const walkCode = (d) => {
    const full = path.join(ROOT, d);
    if (!existsSync(full) || !statSync(full).isDirectory()) return;
    for (const n of readdirSync(full)) {
      if (n === "node_modules" || n === "dist" || n === "build") continue;
      const f = path.join(full, n);
      if (statSync(f).isDirectory()) walkCode(path.join(d, n));
      else if (/\.(ts|tsx|js|jsx|mjs)$/.test(n) && !/\.test\./.test(n)) codeFiles.push(f);
    }
  };
  for (const d of codeDirs) walkCode(d);
  const code = codeFiles.map((f) => readFileSync(f, "utf8")).join("\n");
  const readAnywhere = (token) => {
    const esc = token.replace(/[-]/g, "\\-");
    const inSheet = new RegExp(esc + "(?![a-z0-9-])", "g");
    const writes = new RegExp(esc + "\\s*:", "g");
    const sheetReads = [...text.matchAll(inSheet)].length - [...text.matchAll(writes)].length;
    return sheetReads > 0 || new RegExp(esc + "(?![a-z0-9-])").test(code);
  };
  const unread = [...defined]
    .filter((t) => !readAnywhere(t))
    .filter((t) => ![...UNREAD_OK.keys()].some((k) => t.startsWith(k)))
    .sort();
  totalUnread += unread.length;
  ok(
    `${ui}: every declared token is READ by something`,
    unread.length === 0,
    unread.length
      ? `${unread.length} declared and never read — a name a reader has to check for nothing:\n    ${unread.join("\n    ")}\n  Delete it, or add it to UNREAD_OK with the reason it stays.`
      : "",
  );
}

// ── the SAME rule for the code that READS tokens at runtime (round 235) ─────────────────────────────────────
// This check covered stylesheets only, so nothing noticed that BOTH particles.ts files still ask for the
// RETIRED aura palette:
//
//     pick("--aura-1", 190), pick("--aura-3", 280), pick("--aura-4", 330)
//
// The aura tokens went in the rebrand and globals.css says so in a comment ("used to sit in this slot"); the
// READERS stayed. `pick` falls back when a token reads empty, so nothing broke — the particles quietly draw
// the three PRE-REBRAND hues (190 cyan, 280 violet, 330 pink) instead of the brand's.
//
// THE PATTERN IS DELIBERATELY NARROW. A bare "--something" string is not a token reference: `--json` and
// `--auth` are command-line flags and `--ds-neutral` is a prefix in prose, and a naive scan of the sources
// reported all three. Only calls that ask the DOM for a custom property count.
const ROOT_DIR = fileURLToPath(new URL("../../", import.meta.url));
function walk(dir, out = []) {
  let entries = [];
  try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return out; }
  for (const e of entries) {
    if (e.name === "node_modules" || e.name === "dist" || e.name === "target") continue;
    const full = path.join(dir, e.name);
    if (e.isDirectory()) walk(full, out);
    else out.push(full);
  }
  return out;
}
const ALL_SHEETS = walk(path.join(ROOT_DIR, "agent/resources/panel-react/src/styles"))
  .concat(walk(path.join(ROOT_DIR, "gateway/ui/src/styles")))
  .filter((f) => f.endsWith(".css"));
const ALL_DEFINED = new Set();
for (const f of ALL_SHEETS) {
  for (const m of readFileSync(f, "utf8").matchAll(/(--[a-z0-9-]+)\s*:/g)) ALL_DEFINED.add(m[1]);
}
const RUNTIME_SOURCES = walk(path.join(ROOT_DIR, "gateway/ui/src"))
  .concat(walk(path.join(ROOT_DIR, "agent/resources/panel-react/src")))
  .filter((f) => /\.(ts|tsx|js|jsx)$/.test(f) && !/\.test\./.test(f));
// THE LITERALS, NOT THE CALLS. The first version of this looked for a token name inside
// `getPropertyValue("--x")` and found ZERO references in 117 files — because particles.ts passes the name to a
// helper (`pick("--aura-1", 190)`) and the call site never sees it. The floor below caught that immediately,
// which is what the floor is for. So the scan takes every string literal that LOOKS like a token name and
// requires either a definition or one of the two documented exclusions.
const TOKEN_LITERALS = /["'`](--[a-z0-9-]+)["'`]/g;
// NOT TOKENS, and each says why. These are the three a naive scan reported and a reader can check in seconds.
const NOT_A_TOKEN = new Map([
  ["--json", "a command-line flag in prose (the agent's CLI), not a custom property"],
  ["--ds-neutral", "a PREFIX: the text is `--ds-neutral-*`, naming a family rather than one property"],
]);

// WAIVED, WITH REASONS, the same way the sweeps waive what they cannot judge. EMPTY, and the story of what left
// it is the point: `--aura-1/3/4` sat here because the particle field read a palette the rebrand had retired and
// the replacement was a DESIGN decision, recorded in the operator's inbox rather than guessed at. Round 14 of the
// standing goal carried that decision out (the field wears `--brand-grad-a/b` and `--accent` now), so the waiver
// is gone and a regression fails here like any other undefined token.
const WAIVED_RUNTIME = new Map([]);
{
  const dead = [];
  const found = new Set();
  let seen = 0;
  for (const file of RUNTIME_SOURCES) {
    const text = readFileSync(file, "utf8");
    for (const m of text.matchAll(TOKEN_LITERALS)) {
      const token = m[1];
      seen++;
      found.add(token);
      if (NOT_A_TOKEN.has(token)) continue;
      if (!ALL_DEFINED.has(token) && !WAIVED_RUNTIME.has(token)) dead.push(`${path.relative(ROOT_DIR, file)}: ${token}`);
    }
  }
  // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN, again: floors on both numbers.
  ok("the runtime scan read real files", RUNTIME_SOURCES.length >= 10, `only ${RUNTIME_SOURCES.length} sources`);
  ok("the runtime scan found real references", seen >= 6, `only ${seen} runtime references`);
  ok(
    "every token the CODE reads at runtime is defined somewhere",
    dead.length === 0,
    dead.length ? `${dead.length} undefined — a reader of a removed token falls back silently rather than failing:\n    ${dead.join("\n    ")}` : "",
  );
  console.log(`    ${totalUnread} declared-but-unread across both UIs`);
console.log(`    runtime: ${RUNTIME_SOURCES.length} sources · ${seen} token reads (${[...found].sort().join(" ")}) · ${WAIVED_RUNTIME.size} waived · ${dead.length} undefined`);
}

// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN. The three UIs carry well over a hundred definitions
// between them; a floor here fails loudly if the globs or the patterns stop matching.
ok("the scan read real stylesheets", totalDefs >= 100, `only ${totalDefs} definitions found`);
ok("the scan found real references", totalRefs >= 80, `only ${totalRefs} references found`);

console.log(`\ncss-vars-check: ${failures === 0 ? "all checks passed" : `${failures} FAILED`}`);
process.exit(failures === 0 ? 0 : 1);
