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
];
const EXTS = [".ts", ".mjs", ".js", ".rs", ".ps1", ".nsi", ".bash", ".py"];
const SKIP = ["node_modules", "target", "dist", ".wrangler"];

function walk(dir, out = []) {
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

const files = ROOTS.flatMap((r) => walk(join(ROOT, r))).sort();
const records =
  readFileSync(join(ROOT, "agent", "AGENTS.md"), "utf8") +
  readFileSync(join(ROOT, "docs", "agents", "iteration-coverage.md"), "utf8");

const never = files.filter((f) => !records.includes(f.split("/").pop()));

if (process.argv.includes("--json")) {
  console.log(
    JSON.stringify(
      {
        scope: { roots: ROOTS, extensions: EXTS, skipped: SKIP },
        files: files.length,
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
  console.log(`\nfiles:       ${files.length}`);
  console.log(`never named: ${never.length}`);
  for (const f of never) console.log(`  ${relative(ROOT, f)}`);
  if (!never.length) console.log("  (every source file in scope is named by some round)");
}
