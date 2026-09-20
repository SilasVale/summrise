// THE HARNESS FIXTURES HAVE A CONTRACT, and until round 150 nothing checked it.
//
// The panel's live session list was a FIXED THREE — hardcoded, with no way to ask for another number — so
// the panel's EMPTY state (a fresh install, a device with nothing open) could not be rendered, and had
// therefore never been measured by anything. Round 150 added `?sessions=N` with zero as the point. This
// gate is what keeps it: a one-off measurement is not a guard.
//
// IT READS THE EMITTED HARNESS, not the emitter's source, because the emitted file is what the sweeps
// actually load — the same reason `contrast-probe-check.mjs` compiles the emitted PROBE_SOURCE instead of
// trusting the string in the module. Every pattern here is written against the EMITTED form, where a
// source-level `\s` is already a literal `s` (round 88's trap, and the reason the probe check asserts
// escapes survive).
//
// AND IT TESTS ITSELF. A pattern that matches nothing would pass silently, so each check is run twice: once
// against the real harness, and once against an in-memory copy with the behaviour broken on purpose. A
// check that does not fail on the broken copy is not a check.
import { execFileSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..", "..");
const EMITTER = path.join(ROOT, "agent/scripts/panel-render-audit.mjs");
const OUT = "/tmp/panel-render-audit/panel-harness.html";

let failures = 0;
const ok = (name, cond, detail = "") => {
  if (cond) console.log(`ok: ${name}`);
  else {
    failures++;
    console.log(`FAIL: ${name}${detail ? ` — ${detail}` : ""}`);
  }
};

// The emitter writes the harness and exits 2 on success (its own convention).
try {
  execFileSync("node", [EMITTER], { stdio: "pipe" });
} catch (e) {
  if (e.status !== 2) {
    console.log(`FAIL: the harness emitter exited ${e.status}, not 2 — it did not finish`);
    process.exit(1);
  }
}
if (!existsSync(OUT)) {
  console.log(`FAIL: the emitter wrote no harness at ${OUT}`);
  process.exit(1);
}
const html = readFileSync(OUT, "utf8");
ok("the emitted harness is a real document", html.length > 200_000, `${html.length} bytes`);

/** The contract the fixtures must keep, as predicates over the emitted text. Each returns true when the
 *  BEHAVIOUR is present. The mutations below break one behaviour each. */
const CHECKS = [
  {
    name: "?sessions=N is honoured (the empty state is renderable)",
    test: (h) => /P\.has\('sessions'\)/.test(h) && /parseInt\(P\.get\('sessions'\)/.test(h),
    mutations: [
      { why: "the parameter is ignored again, as before round 150", from: /P\.has\('sessions'\) \? Math\.max\(0, parseInt\(P\.get\('sessions'\), 10\) \|\| 0\) : 3/, to: "3" },
    ],
  },
  {
    name: "zero is a COUNT, not a falsy fallback",
    test: (h) => /Math\.max\(0, parseInt\(P\.get\('sessions'\), 10\) \|\| 0\)/.test(h),
    mutations: [
      { why: "a bare parseInt would treat 0 as absent and render the default three", from: /Math\.max\(0, parseInt\(P\.get\('sessions'\), 10\) \|\| 0\)/, to: "parseInt(P.get('sessions'), 10) || 3" },
    ],
  },
  {
    name: "the default is still THREE, so existing measurements keep their meaning",
    test: (h) => /\? *Math\.max\(0, parseInt\(P\.get\('sessions'\), 10\) \|\| 0\) *: *3/.test(h),
    mutations: [
      { why: "a changed default would silently alter every surface the sweeps already measure", from: /: 3;/, to: ": 7;" },
    ],
  },
  {
    name: "the list is built from the count, not hardcoded",
    test: (h) => /for \(var si = 0; si < liveCount; si\+\+\)/.test(h),
    mutations: [
      { why: "the fixed three came back", from: /for \(var si = 0; si < liveCount; si\+\+\)/, to: "for (var si = 0; si < 3; si++)" },
    ],
  },
  {
    // A STATE WITH NO SURFACE CANNOT BE MEASURED (round 96). `last_exit_code` crossed the wire and the panel
    // draws a chip for a NON-ZERO code — and every seed this harness builds reports no code at all, so without
    // a flag the chip has no rendered surface anywhere. That is the gap rounds 88-92 closed four times running.
    // TWO FLAGS, because the state has three values and the third (exit ZERO) must render as EMPTY.
    name: "?exitfail=1 and ?exitok=1 can express the last command's outcome",
    // ON THE SSH SEED (index 2), AND THE INDEX IS PART OF THE CONTRACT (round 97). With `mode=pending` the first
    // seed holds the question and the second is busy, so a failure planted on either is INVISIBLE — waiting and
    // working both outrank it. Index 2 is the quiet row, which is the one a failure mark has to describe; a
    // mutation that moves it back to [0] fails here rather than silently photographing a waiting diamond.
    test: (h) =>
      /P\.get\('exitfail'\) === '1'/.test(h) &&
      /SESSIONS\[2\]\.last_exit_code = 1;/.test(h) &&
      /P\.get\('exitok'\) === '1'/.test(h) &&
      /SESSIONS\[2\]\.last_exit_code = 0;/.test(h),
    mutations: [
      { why: "the failure flag stopped setting the code, so its only surface renders no failed mark", from: /SESSIONS\[2\]\.last_exit_code = 1;/, to: ";" },
      { why: "exit ZERO stopped being expressible, so the state that must render as EMPTY has no surface", from: /SESSIONS\[2\]\.last_exit_code = 0;/, to: ";" },
      { why: "the failure moved onto the session whose WAITING state hides it (round 97)", from: /SESSIONS\[2\]\.last_exit_code = 1;/, to: "SESSIONS[0].last_exit_code = 1;" },
    ],
  },
  {
    // THE SAME STATE ON THE ACTIVE SESSION (round 98), because that is where the mark is drawn on the accent-filled
    // active tab — the combination whose ink drew 2.16:1 in dark while no surface rendered it. A separate flag, since
    // `?exitfail=1` deliberately plants the failure on a QUIET row to put four states on one page.
    name: "?exitfail=active can put the failure on the session the operator is looking at",
    test: (h) => /P\.get\('exitfail'\) === 'active'/.test(h) && /SESSIONS\[0\]\.last_exit_code = 1;/.test(h),
    mutations: [
      { why: "the active-tab surface stopped rendering a failed mark, so the ink that only fails there is unmeasured again", from: /SESSIONS\[0\]\.last_exit_code = 1;/, to: ";" },
    ],
  },
  {
    // AN ENDPOINT THE PANEL REQUIRES AN ENVELOPE FROM (round 99). `useBootHistory` reads `/api/boots` and treats
    // anything without `ok === true` as a FAILED read, so a stub that omitted it made the Restarts card render
    // "The device did not answer, so its restart history could not be read" on EVERY Settings surface — a false
    // claim about a device that answered perfectly, photographed and judged clean while the card's real content
    // (the summary line and the crash rows) was measured by nothing at all. The device sends the envelope
    // (`api_boots` in agent/src/web/mod.rs); this asserts the FIXTURE does too.
    name: "/api/boots carries the ok envelope the panel's hook requires",
    test: (h) => /J\(\{ ok: true, boots: \[/.test(h),
    mutations: [
      { why: "the envelope is gone, so the card claims the device did not answer", from: /J\(\{ ok: true, boots: \[/, to: "J({ boots: [" },
    ],
  },
  {
    // THE ENVELOPE IS STRUCTURAL NOW (round 100). It was a rule in a comment — "EVERY FIXTURE BELOW NEEDS ok:true
    // ... the omission has cost three rounds" — and the fourth round still happened, because a note is not a
    // mechanism. `J()` merges it into every object body that does not bring its own, so a fixture cannot forget
    // it: a reader that requires `ok === true` gets it, a reader that ignores it is unaffected, and a stub that
    // MEANS to express a failure passes ok:false and is left alone.
    name: "every JSON fixture body carries the ok envelope by default",
    test: (h) => /if \(obj && typeof obj === 'object' && !Array\.isArray\(obj\) && !\('ok' in obj\)\)/.test(h) && /Object\.assign\(\{ ok: true \}, obj\)/.test(h),
    mutations: [
      { why: "the envelope is a rule in a comment again, so the next stub can blame the device for answering", from: /if \(obj && typeof obj === 'object' && !Array\.isArray\(obj\) && !\('ok' in obj\)\) \{\n\s*obj = Object\.assign\(\{ ok: true \}, obj\);\n\s*\}/, to: "" },
    ],
  },
  {
    name: "the status count follows the same number (the fixture cannot contradict itself)",
    test: (h) => /live_sessions: liveCount/.test(h),
    mutations: [
      { why: "a rail showing 3 beside a list showing 0 is a false finding", from: /live_sessions: liveCount/, to: "live_sessions: 3" },
    ],
  },
];

for (const c of CHECKS) {
  ok(c.name, c.test(html), "the emitted harness does not satisfy this any more");
  for (const m of c.mutations) {
    // STALENESS IS JUDGED AGAINST THE REAL HARNESS, not against the mutated copy: when the harness is
    // already broken, the pattern legitimately does not match it, and reporting that as a stale pattern
    // sends the reader after the wrong thing. (Tried the wrong way first in round 151 — under a real
    // mutation two self-tests cried "stale".)
    if (!m.from.test(html)) {
      failures++;
      console.log(`FAIL: the self-test for "${c.name}" cannot run — its pattern does not match the harness at all, so the check above proves nothing (${m.why})`);
      continue;
    }
    const broken = html.replace(m.from, m.to);
    if (broken === html) {
      failures++;
      console.log(`FAIL: the self-test for "${c.name}" changed NOTHING (${m.why})`);
      continue;
    }
    ok(`  self-test: it notices when ${m.why}`, !c.test(broken));
  }
}

console.log(`\nharness-fixture-check: ${failures === 0 ? "all checks passed" : `${failures} FAILED`}`);
process.exit(failures === 0 ? 0 : 1);
