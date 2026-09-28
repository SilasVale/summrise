// ── EVERY TEST FILE IN THIS PACKAGE IS ONE `npm test` RUNS ───────────────────────────────────────
//
// WHY THIS EXISTS. `gateway/package.json` said `"test": "node --test"`, with no pattern, and
// `gateway/ui/` is a SEPARATE PACKAGE living inside it: its own `package.json`, its own
// `package-lock.json`, its own `node_modules`, its own `npm run build` step and its own CI job. A
// bare `node --test` descends into it, so the `gateway` job was also running `ui`'s suite — from a
// directory where `ui`'s dependencies are NOT installed. Measured on `1e46db9a`, the `gateway` job
// went red with:
//
//     Error [ERR_MODULE_NOT_FOUND]: Cannot find package 'jsdom' imported from
//       …/gateway/ui/test/deadCascade.test.mjs
//
// `jsdom` is a devDependency of `gateway/ui` ONLY, and the `gateway` job runs `npm ci` in `gateway/`
// only. So the script is now scoped to this package's own tests.
//
// AND THE SCOPING IS ITSELF A HOLE, WHICH IS WHAT THIS FILE CLOSES. Node's default discovery is two
// rules, and both are quoted verbatim from the v22 and v24 API docs (identical in both):
//
//     NAME      **/*.test.{cjs,mjs,js}   **/*-test.{cjs,mjs,js}   **/*_test.{cjs,mjs,js}
//               **/test-*.{cjs,mjs,js}  **/test.{cjs,mjs,js}
//     LOCATION  **/test/**/*.{cjs,mjs,js}          (+ the {cts,mts,ts} set, stripping being on)
//
// The LOCATION rule is why `test/helpers.mjs` is loaded today even though it declares no tests: any
// file under a directory called `test` is a test file. Scoping to `test/` keeps that rule whole and
// drops the NAME rule everywhere outside it — so a future `gateway/foo.test.mjs`,
// `gateway/scripts/foo.test.mjs` or `gateway/src/foo-test.mjs` would be discovered by NOBODY, and
// the suite would stay green while the file never ran. That is the one failure a scoped discovery
// can produce silently, and this file refuses it.
//
// THE INVARIANT: every file under `gateway/` that Node's default discovery would match is under
// `test/` — except under `gateway/ui/`, which is a separate package judged by its own CI job.
//
// THE SCRIPT'S PATTERN IS A GLOB, NOT A DIRECTORY, AND THAT IS MEASURED RATHER THAN ASSUMED. Both
// the v22 and v24 docs describe `--test`'s arguments as glob patterns ("Glob patterns follow the
// behavior of glob(7) … enclosed in double quotes to prevent shell expansion"); neither documents a
// directory. Measured on Node v22.23.3 and v24.20.0, `node --test test/` is not discovery at all:
//
//     Error: Cannot find module '/tmp/pat/test'      (exit 1 — it loads `test/` as an entry point)
//
// while `node --test "test/**/*.{cjs,mjs,js,cts,mts,ts}"` finds every file under `test/` on both,
// and a pattern that matches nothing is not an error on either. Hence the script in package.json.
//
// MUTATION: printf 'import test from "node:test";\ntest("planted", () => {});\n' > gateway/planted.test.mjs
// RESULT:   exit 1 — and the plant is not merely flagged, it is UNRUN, which is the hole itself:
//           `cd gateway && npm test` on Node 22.23.3 reports `# tests 892` / `# pass 891` /
//           `# fail 1` (the plant would have made it 893), and the one failure is this file:
//
//             not ok 813 - no test file in this package can be skipped by npm test
//               error: |-
//                 1 file(s) under gateway/ would be discovered by a bare `node --test` and is not
//                 under test/ — `npm test` is scoped to test/, so nothing would run it:
//                   planted.test.mjs   (matched **/*.test.{cjs,mjs,js})
//                 Move it into test/, or if it belongs to gateway/ui, into ui/ — which has its own
//                 package.json, its own node_modules and its own CI job.
//
//           The floor test passed in the same run (`ok 812`), which is the point: the scan still read
//           the package, so that was a FINDING and not a broken scan. Deleted, all three pass.
import test from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url)); // …/gateway
const UI = "ui";

/** The NAME rule, quoted from the v22/v24 docs (see the header) — the documented pattern, and its
 *  regex. `{cjs,mjs,js}` is written `(c|m)?(js|ts)` because the docs also match the `{cts,mts,ts}`
 *  set while type stripping is on, which it is by default in both versions. */
const NAME_PATTERNS = [
  ["**/*.test.{cjs,mjs,js}", /\.test\.(c|m)?(js|ts)$/],
  ["**/*-test.{cjs,mjs,js}", /-test\.(c|m)?(js|ts)$/],
  ["**/*_test.{cjs,mjs,js}", /_test\.(c|m)?(js|ts)$/],
  ["**/test-*.{cjs,mjs,js}", /(^|[\\/])test-[^\\/]*\.(c|m)?(js|ts)$/],
  ["**/test.{cjs,mjs,js}", /(^|[\\/])test\.(c|m)?(js|ts)$/],
];
/** The LOCATION rule: anything at any depth under a directory called `test`. */
const LOCATION = /(^|[\\/])test[\\/].*\.(c|m)?(js|ts)$/;

/**
 * Every file under `dir`, relative to `ROOT`. `node_modules` and dot-entries are skipped because the
 * runner skips them — MEASURED, not assumed: a planted `node_modules/dep.test.mjs` and a planted
 * `.hidden.test.mjs` were both absent from a bare `node --test`, while `test/deep/nested.mjs` (a
 * nested directory under `test/`) was present.
 */
function walk(dir, out = []) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    if (e.name === "node_modules" || e.name.startsWith(".")) continue;
    const p = join(dir, e.name);
    if (e.isDirectory()) walk(p, out);
    else out.push(relative(ROOT, p));
  }
  return out;
}

const files = walk(ROOT);
// The separate package is excluded, and the exclusion is EARNED below rather than assumed.
const own = files.filter((f) => f.split(sep)[0] !== UI);

/** `[path, the documented pattern it matched]` for everything a bare `node --test` would discover. */
function discoverable(paths) {
  const found = [];
  for (const f of paths) {
    const named = NAME_PATTERNS.find(([, re]) => re.test(f));
    if (named) found.push([f, named[0]]);
    else if (LOCATION.test(f)) found.push([f, "**/test/**/*.{cjs,mjs,js}"]);
  }
  return found;
}

const discovered = discoverable(own);
const strays = discovered.filter(([f]) => !f.startsWith(`test${sep}`));

// FLOOR — a scan that read nothing is not a clean scan, and this one has three things to read.
test("the scan read the package, its own tests, and the separate package it excludes", () => {
  assert.ok(files.length >= 100, `walked only ${files.length} file(s) under gateway/ — the scan is reading the wrong tree`);
  const ownTests = files.filter((f) => f.startsWith(`test${sep}`));
  assert.ok(
    ownTests.length >= 40,
    `found ${ownTests.length} file(s) under test/ — this package declares ~61, so the walk or the layout moved`,
  );
  assert.ok(discovered.length >= 40, `only ${discovered.length} discoverable file(s) found; the name patterns went stale`);
  assert.ok(
    files.some((f) => f.split(sep)[0] === UI),
    "gateway/ui/ was not walked at all, so the exclusion below is not being tested",
  );
});

// THE CONTRACT — the hole the scoping opens, refused by name.
test("no test file in this package can be skipped by npm test", () => {
  const named = strays.map(([f, p]) => `  ${f}   (matched ${p})`).join("\n");
  assert.deepEqual(
    strays,
    [],
    `${strays.length} file(s) under gateway/ would be discovered by a bare \`node --test\` and is not under ` +
      `test/ — \`npm test\` is scoped to test/, so nothing would run it:\n${named}\n` +
      `Move it into test/, or if it belongs to gateway/ui, into ui/ — which has its own package.json, ` +
      `its own node_modules and its own CI job.`,
  );
});

// THE OTHER HALF OF THE BOUNDARY, PINNED HERE BECAUSE THIS FILE'S OWN FIX COULD BREAK IT. `gateway`'s
// job no longer reaches ui's suite, so ui's job MUST keep discovering all of it: a scoped ui script
// would drop `ui/test/deadCascade.test.mjs` from BOTH jobs, and the console's cascade gate would run
// nowhere — green, and proving nothing.
test("gateway/ui is a separate package, and its own job still discovers every test in it", () => {
  let pkg;
  try {
    pkg = JSON.parse(readFileSync(join(ROOT, UI, "package.json"), "utf8"));
  } catch {
    assert.fail(
      `gateway/${UI}/package.json is gone or unreadable, so excluding ${UI}/ from this package's discovery is no ` +
        `longer earned — either restore it or fold those tests into test/.`,
    );
  }
  const cmd = pkg.scripts?.test;
  assert.ok(cmd, `gateway/${UI}/package.json declares no \`test\` script, so nothing runs the console's suite`);
  assert.match(cmd, /^node --test(\s|$)/, `gateway/${UI}'s test script is "${cmd}", not a \`node --test\` run`);
  const positional = cmd
    .split(/\s+/)
    .slice(2)
    .filter((a) => !a.startsWith("-"));
  assert.deepEqual(
    positional,
    [],
    `gateway/${UI}'s test script is scoped to ${positional.join(" ")}. It must stay UNscoped: this package's ` +
      `discovery no longer reaches ${UI}/, so a scoped ${UI} script would leave its tests running in NO job.`,
  );
});
