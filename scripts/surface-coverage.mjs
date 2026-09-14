#!/usr/bin/env node
// surface-coverage — how many source files exist, and which no round has ever named.
//
// WHY THIS EXISTS. The ledger's "Never-examined surfaces" table opens with
// "**121 files, 15 never named by any round**" — and states no scope and ships no
// instrument, so the number cannot be checked. Round 228 measured 118 with its own
// definition and could not reconcile the difference, because there was nothing to
// reconcile against. **A count whose scope is unstated is a recollection, not a
// measurement.**
//
// This is an OPS TOOL, deliberately NOT a CI gate — the same posture as
// `scripts/model-drift.mjs`. The number that matters ("never named") CHANGES every
// time a file is added, so asserting it would fail on a normal working day; what is
// worth having is the ability to ask the question and get an answer with its scope
// attached.
//
// Usage:  node scripts/surface-coverage.mjs [--json]

import { readdirSync, readFileSync, statSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));

/** THE SCOPE, STATED — this is the thing the ledger's number was missing. */
const ROOTS = [
  "gateway/src",
  "index/src",
  "agent/src",
  "agent/vale-command-core/src",
  "proxies/zen-go-proxy/src",
  "proxies/zen-us-proxy/src",
  // api-relay has NO src/ — round 229 listed `proxies/api-relay/src`, which does not
  // exist, and walk() skipped it silently, so the whole subsystem went uncounted while
  // the output still read "never named: 0". Its real directories are these two, and
  // they hold 17 files — 5 handlers plus the vrelay server's entry/routing, which
  // round 223 DEPLOYED without any round ever naming them.
  "proxies/api-relay/api",
  "proxies/api-relay/server",
  "extension",
  // Round 234: `scripts/` holds the release/publish tooling AND every check script CI
  // runs (`model-drift-check`, `contrast-probe-check`, `panel-audit-skip-check`,
  // `e2e-only-check`, `token-contract-check`, `scan-dups-check.py`, the six bash
  // suites), and it was never in this scope at all — so "never named" said nothing
  // about 19 files. Adding it moves the count 135 -> 154 and the never-named list
  // 12 -> 10, which is the point: a scope is a claim, and this one was narrower than
  // what a reader of "every source file in scope is named" would infer.
  "scripts",
  // ── round 236: TESTS JOIN THE SCOPE ────────────────────────────────────────────
  // Round 235 measured 261 files outside it and round 236 classified them: 124 are
  // plain test files plus ~34 more under the panel SPA's `__tests__`, i.e. the bulk of
  // the omission was the one category round 231 proved matters most. `agent/tests/`
  // holds THIS LOOP'S OWN INSTRUMENTS (`ledger_head.rs`, `adr_allocation.rs`,
  // `gateway_code_contract.rs`, `module_map.rs`) and `gateway/test/` holds the 872
  // tests the journal quotes every round — and neither had ever been in scope.
  "gateway/test",
  "agent/tests",
  "index/test",
  "extension/test",
  "proxies/zen-go-proxy/test",
  "proxies/zen-us-proxy/test",
  "proxies/api-relay/api/test",
  "proxies/api-relay/server/test",
  "agent/resources/panel-react/src",
  "agent/vale-agent-npm",
  "gateway/ui",
  "agent/vale-desktop-electron",
  "agent/deploy",
  "agent/scripts",
];

/**
 * DIRECTORIES THAT ARE DELIBERATELY OUT, EACH WITH A REASON — so the next reader does
 * not have to re-derive the boundary, and so "not in scope" is a DECISION rather than
 * an accident. Round 235's lesson was that a list cannot notice what it omits; this is
 * the other half — an omission stated is no longer an omission.
 */
const EXCLUDED = [
  ["gateway/public/code", "GENERATED — the code-viewer mirror; its obligation is round 211's mirror test, not 'has a round named it'"],
  ["agent/vendor-portable-pty", "VENDORED third-party source"],
  ["agent/deploy/retired", "RETIRED installer scripts (deploy/retired/)"],
  ["agent/resources/panel", "BUILD OUTPUT (panel.js is embedded at compile time)"],
  ["brand", "images/assets only — no source extensions in scope"],
];
// `.tsx` IS DELIBERATELY NOT HERE, and it is the one boundary in this file that is stated
// rather than enforced. Round 257 repaired the silent half (see `excludedDir`): the panel
// SPA's `.ts` files are now counted as `ROOTS` always claimed. Its React components are
// `.tsx`, so they are NOT — which means "the panel SPA is in scope" is true of its scripts
// and its hooks and false of its components. Adding `.tsx` is a SCOPE DECISION, not a bug
// fix, and it is recorded as one: it would move the headline 349/363 -> 435/449 and the
// never-named queue 87 -> 155, i.e. most of two React SPAs that no round has ever named.
const EXTS = [".ts", ".mjs", ".js", ".rs", ".ps1", ".nsi", ".bash", ".py"];

/**
 * Is this repo-relative path inside a deliberately excluded DIRECTORY?
 *
 * SEPARATOR-AWARE, and that is the whole point (round 257). The rule used to be
 * `rel.startsWith(dir)`, a bare string prefix — and `agent/resources/panel` (BUILD OUTPUT,
 * excluded) is a prefix of `agent/resources/panel-react` (the panel SPA's SOURCE, listed in
 * ROOTS since round 236). So the panel SPA was silently excluded from a scope that claimed to
 * contain it: `ROOTS` printed `agent/resources/panel-react/src` on every run while not one of
 * its ~60 TypeScript files was counted, which is precisely the "a directory claiming both"
 * failure round 236 fixed one function over — an exclusion that answers a question nobody
 * asked it.
 *
 * The denominator already appended `"/"`; the WALK did not, so the two halves of one rule
 * disagreed and only the permissive one was ever visible in the output.
 */
const excludedDir = (rel) => EXCLUDED.some(([d]) => rel === d || rel.startsWith(d + "/"));
const SKIP = ["node_modules", "target", "dist", ".wrangler"];

/**
 * THE FILES THE **REPOSITORY** CONTAINS — git's index, not whatever is on this disk.
 *
 * WHY THIS EXISTS (round 256). `repositoryFiles` was a plain walk of the working
 * directory, so a local scratch file counted. Four did: `ecosystem.config.js` and the
 * three `restart-plugin*.js` helpers sit in the repo root, untracked and never
 * committed, so THIS box measured a denominator of 365 while a CI checkout of the same
 * commit measured 361 — and `agent/tests/coverage_numbers.rs` asserts the ledger's
 * headline against whatever the tool says. **The gate therefore passed here and failed
 * in CI on every run**, with the ledger's number correct for exactly one of the two
 * environments. A count that depends on which machine asks is not a measurement of the
 * repository, and the ledger's own sentence says "the repository's N non-excluded
 * files" — so the repository is what gets counted.
 *
 * `null` (no git, or no checkout) falls back to the walk and SAYS SO in the output:
 * an unstably-scoped number is worse than a stated one, but a silently narrower scope
 * is worse than both — the failure round 229 recorded, where an unreadable root read
 * as an empty directory and the verdict stayed clean.
 */
function trackedInRepo() {
  try {
    const out = execFileSync("git", ["ls-files", "-z"], {
      cwd: ROOT,
      maxBuffer: 128 * 1024 * 1024,
    });
    return new Set(out.toString("utf8").split("\0").filter(Boolean));
  } catch {
    return null;
  }
}
const TRACKED = trackedInRepo();
/** A walked path is kept only when the repository actually holds it. */
const inRepo = (p) => !TRACKED || TRACKED.has(relative(ROOT, p));

function walk(dir, out = []) {
  // EXCLUDED applies to the WALK, not only to the denominator. Round 236 first put
  // this check inside the `catch` below, where it ran only on a READ ERROR — so
  // `agent/deploy/retired/*` was counted as never-named while the same output
  // declared that directory deliberately out of scope. A directory claiming both.
  if (excludedDir(relative(ROOT, dir))) return out;
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch (e) {
    // A ROOT that cannot be read is an ERROR, never an empty directory. Round 229
    // shipped this with `catch { return out }`, and the measurement that exposed it is
    // in the journal: typo'ing one root took `files` from 118 to 77 while
    // `never named` stayed 0 — a plausible, LOWER number with a clean verdict, which
    // is the one failure mode an instrument must not have.
    if (ROOTS.includes(relative(ROOT, dir))) {
      console.error(`FATAL: root in scope does not exist or is unreadable: ${relative(ROOT, dir)}`);
      process.exit(2);
    }
    return out; // a nested directory may legitimately vanish mid-walk
  }
  for (const e of entries) {
    if (SKIP.includes(e.name)) continue;
    const p = join(dir, e.name);
    if (e.isDirectory()) walk(p, out);
    else if (EXTS.some((x) => e.name.endsWith(x))) out.push(p);
  }
  return out;
}

// DEDUPE: round 236 added `proxies/api-relay/api/test` alongside `.../api`, and
// `.../server/test` alongside `.../server`, so the walk reached those files twice and
// `files.length` counted them twice — inflating the numerator past the denominator.
// The run's own self-check (`a + b MUST equal c`) caught it on its first execution.
const files = [...new Set(ROOTS.flatMap((r) => walk(join(ROOT, r))))]
  .filter(inRepo)
  .sort();

/**
 * WHAT THIS SCOPE DOES **NOT** COVER, COMPUTED RATHER THAN IMPLIED.
 *
 * Through rounds 229-234 the verdict below ("every source file in scope is named") was
 * read as a statement about the repository, and it never was: the scope covered 154 of
 * the repository's 415 source files. Round 234 closed `scripts/` by adding it, and that
 * is the wrong shape of fix — a LIST cannot notice what it omits, so the next directory
 * would have been found the same way. This measures the omission instead: the run now
 * prints how much of the repository it looked at, and names the top-level directories it
 * did not, so "in scope" can never again be read as "in the repository".
 */
function repositoryTotals() {
  const all = [];
  const stack = [ROOT];
  while (stack.length) {
    const dir = stack.pop();
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const e of entries) {
      if (SKIP.includes(e.name)) continue;
      const p = join(dir, e.name);
      if (e.isDirectory()) stack.push(p);
      else if (EXTS.some((x) => e.name.endsWith(x))) all.push(p);
    }
  }
  return all;
}

const everything = repositoryTotals()
  .filter(inRepo)
  .filter((f) => !excludedDir(relative(ROOT, f)));
const inScope = new Set(files);
const outside = everything.filter((f) => !inScope.has(f));
const outsideByTop = {};
for (const f of outside) {
  const rel = relative(ROOT, f);
  const top = rel.includes("/") ? rel.split("/")[0] : "(repo root)";
  outsideByTop[top] = (outsideByTop[top] || 0) + 1;
}
const records =
  readFileSync(join(ROOT, "agent", "AGENTS.md"), "utf8") +
  readFileSync(join(ROOT, "docs", "agents", "iteration-coverage.md"), "utf8");

const never = files.filter((f) => !records.includes(f.split("/").pop()));

if (process.argv.includes("--json")) {
  console.log(
    JSON.stringify(
      {
        scope: {
          roots: ROOTS,
          extensions: EXTS,
          skipped: SKIP,
          countedOver: TRACKED ? "git ls-files" : "working-directory",
        },
        files: files.length,
        repositoryFiles: everything.length,
        notInScope: outsideByTop,
        excluded: Object.fromEntries(EXCLUDED),
        neverNamed: never.length,
        neverNamedList: never.map((f) => relative(ROOT, f)),
      },
      null,
      2,
    ),
  );
} else {
  console.log("scope:");
  for (const r of ROOTS) console.log(`  ${r}  (${EXTS.join(" ")})`);
  // THE SCOPE'S OWN SCOPE, PRINTED. Both counts are over `git ls-files`, so this
  // number is a property of the COMMIT and not of the machine that asked — see
  // trackedInRepo(). Untracked working-directory files are named as excluded rather
  // than silently dropped, because "not counted" and "not there" are different facts.
  console.log(
    TRACKED
      ? "  counted over: git ls-files (the COMMIT, so a CI checkout measures the same)"
      : "  counted over: the working directory (no git checkout — this number may not travel)",
  );
  console.log(`\nfiles in scope: ${files.length} of ${everything.length} in the repository ` +
    `(${((files.length / everything.length) * 100).toFixed(0)}%)`);
  console.log(
    `NOT in scope: ${outside.length} of ${everything.length} \u2014 ` +
      `${files.length} + ${outside.length} = ${files.length + outside.length} MUST equal ${everything.length}`,
  );
  console.log("  by top-level directory:");
  for (const [k, v] of Object.entries(outsideByTop).sort((a, b) => b[1] - a[1])) {
    console.log(`  ${k}  (${v})`);
  }
  console.log("\nDELIBERATELY out of scope (stated, not omitted):");
  for (const [dir, why] of EXCLUDED) console.log(`  ${dir} \u2014 ${why}`);
  console.log(`\nnever named: ${never.length}`);
  for (const f of never) console.log(`  ${relative(ROOT, f)}`);
  if (!never.length) console.log("  (every source file in scope is named by some round)");
}
