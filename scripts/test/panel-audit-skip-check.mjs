// ── THE MUTATION THAT MUST FAIL THIS GATE (moved here from the ledger table, landing 4b) ──
// Read this when you change this file: the mutation is how you find out whether the gate can still
// fail at all. A gate that cannot be broken is worse than no gate.
//
// MUTATION: make the audit `exit(0)` on a skip, OR delete the `PROBE_SOURCE` import its measuring half evaluates, OR put its old local `cr < 4.5` back
// RESULT:   exit 1 each way: the first names the skip/pass distinction; the other two read the SOURCE, because that half runs only where a browser exists — and it had never run anywhere (CI takes the emit path, which exits 2 first) until round 265 imported the probe it was calling by a name that no longer existed

// A SKIP MUST NOT LOOK LIKE A PASS.
//
// `panel-render-audit.mjs` needs a Playwright runtime (`SUMMRISE_BROWSER_HELPER`). Where
// there is none it still emits its harness — deliberately, because a check that can
// only run in one environment quietly stops running — but it used to exit 0, and its
// own message admitted the consequence: "a caller watching only the exit code reads
// this skip as a pass".
//
// That is the same defect this repo fixed twice already: round 33's "a check that
// reads nothing must not report success", and round 46's harness that could not tell
// an unmocked request from an empty page. Here the only channel a caller is
// guaranteed to read is the exit code, so the convention is pinned:
//
//   0  the audit RAN and found nothing
//   1  the audit RAN and found failures
//   2  the audit DID NOT RUN
import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const SCRIPT = path.join(ROOT, "agent", "scripts", "panel-render-audit.mjs");

function run(env) {
  return spawnSync(process.execPath, [SCRIPT], {
    encoding: "utf8",
    env: { ...process.env, ...env },
    timeout: 60000,
  });
}

test("with no Playwright runtime the audit exits 2, not 0", () => {
  const env = { ...process.env };
  delete env.SUMMRISE_BROWSER_HELPER;
  const r = spawnSync(process.execPath, [SCRIPT], { encoding: "utf8", env, timeout: 60000 });
  assert.equal(
    r.status,
    2,
    `expected exit 2 (DID NOT RUN); got ${r.status}. A skip that exits 0 is read as a pass ` +
      `by anything watching only the exit code. stdout: ${r.stdout?.slice(0, 200)}`,
  );
});

test("the skip SAYS it did not run, in words", () => {
  const env = { ...process.env };
  delete env.SUMMRISE_BROWSER_HELPER;
  const r = spawnSync(process.execPath, [SCRIPT], { encoding: "utf8", env, timeout: 60000 });
  const out = (r.stdout || "") + (r.stderr || "");
  assert.match(out, /DID NOT RUN/i, "the skip must say so in words, not only in the exit code");
  assert.match(out, /SKIP, not a pass/i, "and must name the distinction explicitly");
});

test("the three exit codes are distinct and documented in the script", async () => {
  const { readFileSync } = await import("node:fs");
  const src = readFileSync(SCRIPT, "utf8");
  // The convention has to be WRITTEN WHERE THE CODES ARE SET, or the next person to
  // add an exit path has nothing to follow.
  assert.match(src, /2\s+the audit DID NOT RUN|audit DID NOT RUN/, "exit 2 is not documented");
  assert.match(src, /exit\(2\)/, "nothing actually exits 2");
  void run;
});

// ── THE HALF THAT NEEDS A BROWSER, CHECKED AS SOURCE (round 265) ─────────────────────────────────────────
// Every test above pins the SKIP. The audit's other half — the one that measures — runs only where a Playwright
// runtime exists, and there it died on `PROBE is not defined`: the probe moved to `lib/contrast-probe.mjs` (this
// file's own emit message says so) and the audit kept calling a bare `PROBE`. Nothing in the repository could
// fail on it: CI takes the emit path, which exits 2 before that line, and the tests above pin that exit. The
// crash was reached for the first time by a DEVICE, and it read like the audit being broken rather than like a
// missing import. So the check reads the source: every name handed to `page.evaluate()` must exist in the file.
test("every name the audit evaluates in the page is defined in the file", async () => {
  const { readFileSync } = await import("node:fs");
  const src = readFileSync(SCRIPT, "utf8");
  const defined = new Set();
  for (const m of src.matchAll(/import\s*\{([^}]+)\}\s*from/g)) {
    for (const part of m[1].split(",")) {
      const name = part.trim().split(/\s+as\s+/).pop().trim();
      if (name) defined.add(name);
    }
  }
  for (const m of src.matchAll(/(?:const|let|var|function)\s+([A-Za-z_$][\w$]*)/g)) defined.add(m[1]);
  // BARE IDENTIFIERS ONLY: `page.evaluate((sels) => …, REQUIRED)` passes a function, and there is nothing to look
  // up for it — the regex requires the argument to start with a name, so those calls are not matched at all.
  const evaluated = [...src.matchAll(/page\.evaluate\(\s*([A-Za-z_$][\w$]*)\s*[,)]/g)].map((m) => m[1]);
  assert.ok(
    evaluated.length >= 2,
    `found ${evaluated.length} page.evaluate(<name>) call(s) — a scan that matches nothing proves nothing`,
  );
  for (const name of new Set(evaluated)) {
    assert.ok(
      defined.has(name),
      `page.evaluate(${name}) — ${name} is neither imported nor declared in this file. This is the crash a device reaches and CI cannot: the emit path exits before it.`,
    );
  }
});

test("the audit judges contrast with the shared probe, not with a local literal", async () => {
  const { readFileSync } = await import("node:fs");
  const { decomment } = await import("./lib/decomment.mjs");
  // COMMENTS ARE STRIPPED FIRST (the lesson `retired-colours-check` records from its own first run): the file's own
  // explanation of this repair QUOTES the defect — "not a local `cr < 4.5`" — and a scan that flags its own
  // documentation would have to be switched off. What is checked is the CODE.
  const src = decomment(readFileSync(SCRIPT, "utf8"));
  // A local `cr < 4.5` is what this file used to do, and it was wrong in two directions at once: it asked 4.5 of
  // GRAPHIC rows (which need 3, so a healthy ring was reported as a failure) and it counted a row nobody could
  // read as a pass — every comparison against null is false. `failures()` knows `need` and `inactive`;
  // `unmeasurable()` is the count that must be zero for a reading to mean anything.
  assert.doesNotMatch(
    src,
    /cr\s*<\s*4\.5/,
    "a local `cr < 4.5` asks text's threshold of graphics and counts an unreadable row as passing — use the shared helpers",
  );
  assert.match(src, /contrastFailures\(rows\)/, "the shared judgement must actually be applied");
  assert.match(src, /unmeasurable\(rows\)/, "an unmeasurable row must be reported, not silently counted as a pass");
});
