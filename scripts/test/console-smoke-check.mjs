// Run EVERY console render smoke, and refuse to report success having run none.
//
// WHY THIS EXISTS. Four render smokes live in gateway/ui/ and CI ran exactly one of them —
// `render-smoke.mjs`, written inline in ci.yml. `overview-render-smoke.mjs` recorded its own neglect
// in a comment: "Nothing runs this smoke in CI (ci.yml runs render-smoke.mjs only), which is why the
// drift survived." The drift was that `.ov-firstrun` is a class the view has NEVER rendered, so two of
// its checks — including scene 3, the honesty rule the whole file exists for — were asserting on the
// empty string and could not fail. A smoke that runs nowhere is worse than no smoke, because its green
// is read as coverage.
//
// THE SET IS DISCOVERED, NOT LISTED: every `*-render-smoke.mjs` next to the views is run, so a new one
// is covered the moment it exists and this file cannot become a second list that drifts. The floor
// below is what tells "the tree moved" from "there are no smokes any more".
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const UI = path.join(ROOT, "gateway", "ui");

const smokes = readdirSync(UI)
  // `render-smoke.mjs` has no screen prefix, so the suffix — not `*-render-smoke.mjs` — is the rule.
  .filter((f) => f.endsWith("render-smoke.mjs"))
  .sort();
if (smokes.length < 4) {
  console.error(
    `  FAIL read only ${smokes.length} console smoke(s) from gateway/ — the tree moved, so this proves nothing`,
  );
  process.exit(1);
}

const html = readFileSync(path.join(ROOT, "gateway/public/index.html"), "utf8");
const bundle = (html.match(/index-[^"]*\.js/) || [])[0];
if (!bundle) {
  console.error("  FAIL no index-*.js in gateway/public/index.html — the console is not built");
  process.exit(1);
}
const built = path.join(ROOT, "gateway/public/assets", bundle);

let failed = 0;
for (const s of smokes) {
  try {
    execFileSync("node", [s, built], { cwd: UI, stdio: "pipe" });
    console.log(`  ok    ${s}`);
  } catch (e) {
    failed += 1;
    console.log(`  FAIL  ${s}`);
    const out = String(e.stdout || "");
    for (const line of out.split("\n").filter((l) => /✗|FAIL|not ok/.test(l)).slice(0, 4)) {
      console.log(`          ${line.trim()}`);
    }
  }
}

console.log(`\n  ${smokes.length - failed}/${smokes.length} console smoke(s) passed`);
process.exit(failed > 0 ? 1 : 0);
