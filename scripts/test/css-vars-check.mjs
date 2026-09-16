// EVERY `var(--token)` MUST RESOLVE. An undefined custom property does not fall back to something
// sensible — it makes the WHOLE DECLARATION invalid at computed-value time, silently. That is how the
// extension's keyboard focus ring came to be invisible on every control (round 137): options.css styled
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
  extension: ["extension/options", "extension"],
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

let totalDefs = 0;
let totalRefs = 0;
for (const [ui, dirs] of Object.entries(UIS)) {
  const files = cssFiles(dirs);
  ok(`${ui}: stylesheets found`, files.length > 0, "no .css under " + dirs.join(", "));
  const text = files.map((f) => readFileSync(f, "utf8")).join("\n");
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
}

// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN. The three UIs carry well over a hundred definitions
// between them; a floor here fails loudly if the globs or the patterns stop matching.
ok("the scan read real stylesheets", totalDefs >= 100, `only ${totalDefs} definitions found`);
ok("the scan found real references", totalRefs >= 80, `only ${totalRefs} references found`);

console.log(`\ncss-vars-check: ${failures === 0 ? "all checks passed" : `${failures} FAILED`}`);
process.exit(failures === 0 ? 0 : 1);
