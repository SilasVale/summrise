// THE LIVE PANEL, MEASURED. Every design measurement this suite has ever made was against the HARNESS — a
// 366-node page with three sessions — and round 120 recorded that some numbers genuinely differ on the device
// (panel-density strip widths measure 0-185px in a browser and 211px on the device). Round 167 ran the probe
// against the real panel for the first time: 1488 root nodes, 14 live sessions, and zero failing text or
// graphic rows. This script is that measurement, made repeatable, because a one-off check is not a guard —
// the rule this suite has applied to the empty state, the message tones, the target sizes and the type floor.
//
// IT RUNS ON THE DEVICE, because the panel is on the agent's own loopback: `127.0.0.1:18080`. The emitter
// writes a standalone script; run it with the device's bundled Playwright runtime (see the sweeps' recipe:
// `browser_run_script`, which is how every other emitted script here is executed).
//
// THE TOKEN IS READ FROM THE DEVICE'S OWN CONFIG IN-PROCESS and never printed — not to stdout, not into the
// report. If the config moves, the script says where it looked rather than failing silently.
//
// ── HOW TO RUN IT ON A DEVICE (round 228 — this cost four rounds to work out, so it is written down) ──────────────────
//
//   1. GET THE TREE THERE. The console's code viewer mirrors these instruments, so a device can fetch them from the CDN —
//      which is the ONLY channel that worked: raw githubusercontent times out on lib/design-sweep.mjs (116 KB), the device
//      has no `git`, and the relay inbox wants an admin token.
//
//          https://<dist-host>/code/files/instruments/live-panel-probe.mjs
//          https://<dist-host>/code/files/instruments/lib/design-sweep.mjs
//          https://<dist-host>/code/files/instruments/lib/contrast-probe.mjs
//
//      (All three, into a directory with `lib/` beside the probe — this PROBE's imports are relative and it needs nothing
//      else, so it can live anywhere. THE SWEEPS CANNOT: they infer their repo root from their own location, so
//      `panel-design-sweep.mjs` and `panel-render-audit.mjs` must sit at `<somewhere>/agent/scripts/` with `lib/` beside them,
//      or they look for `<root>/agent/resources/panel/panel.css` in a place that does not exist. Measured: from
//      `D:\Vale\etc` the emitter died on `D:\agent\resources\panel\panel.css`, and from
//      `D:\vale-dev\agent\scripts` it worked. `manifest.json` lists everything.
//   2. EMIT ON THE DEVICE, with its bundled node:
//          node live-panel-probe.mjs --emit > probe-live.js
//   3. RUN IT THROUGH `browser_run_script`, and do NOT re-transcribe it — that runner INJECTS the two environment pieces the
//      emitted script needs, one of which does not exist as a file anywhere:
//
//          require("D:\\path\\to\\probe-live.js");
//
//      THE TRAP: run it from a terminal instead and it dies with `The "id" argument must be of type string. Received
//      undefined` — that is `require(undefined)`, because `VALE_BROWSER_HELPER` is set by the RUNNER and by nothing else. Two
//      rounds were spent guessing environment variables at a terminal before anyone read the emitted script's first line.
//   4. A GREEN RUN looks like this (the operator's device, 2026-09-22, both densities): `textFailing: []`,
//      `graphicFailing: []`, `unmeasurable: 0`, `marks.collisions: []`, `marks.ringFill: []`, `errors: []`, and
//      `verdict.ok: true`. A NON-ZERO `unmeasurable` is not a pass: an absence is not evidence until the instrument is shown
//      to see.
//
// ── WHAT THE SWEEPS NEED ON TOP OF THAT (rounds 229-248, the same recipe plus four things) ──────────────────────────────
//
//   A. THE LAYOUT, as above: `<root>/agent/scripts/`. This is the one that cost the most — the probe hid it, because the
//      probe is the only instrument here that does not care where it lives.
//   B. THE STAMP, handed over: `VALE_HARNESS_STAMP="<len>-<sha256[:12]>"`. Both emitters read it, so the harness and the
//      sweep that judges it agree by construction instead of both happening to read the same file — which on a device
//      neither of them can. Compute it from the sheet the device serves:
//          curl -s http://127.0.0.1:18080/panel/panel.css   →   length + sha256[:12]
//      Without it the sweep reports `harnessStale: true` and refuses to vouch for its own numbers. That guard is right; give
//      it something true to compare.
//   C. THE AGENT MUST ANSWER CHROME'S PRIVATE NETWORK CHECK (1.2.447+). The sweeps serve their harness from
//      `http://vale.test` — a PUBLIC name — and a public-origin page is refused a request to 127.0.0.1 even when the response
//      says `Access-Control-Allow-Origin: *`: "the request client is not a secure context and the resource is in
//      more-private address space (?local?)". The assets carry `Access-Control-Allow-Private-Network: true` and the OPTIONS
//      preflight is answered. The live probe never met this because ITS page is served from 127.0.0.1 — the same address
//      space. If a sweep renders an empty page and times out on `.side-row, .dtab, .tab`, READ THE PAGE'S CONSOLE before
//      theorising: it says this in one line, and four wrong theories in a row did not.
//   D. ONE STAGE PER `browser_run_script` CALL, AND KNOW WHAT A CALL CANNOT DO. Its timeout is shorter than a full sweep:
//      emit in one call, run in another, read the report file in a third. A call that does all three dies mid-way and leaves
//      a report from the PREVIOUS run, which reads exactly like a real result.
//
//      AND A PASS LONGER THAN A CALL CANNOT BE RUN FROM THAT RUNNER AT ALL (round 253, measured). The sweep writes its report
//      at the END, and when the call is killed the process goes with it, so a `--passes=pages` run (48 surfaces) leaves
//      NOTHING — not a partial report, not a line. Starting it `detached` with `.unref()` does not help: this device kills a
//      runner's children when the call ends (the same job-object behaviour that ate the relay). Measured: a detached run
//      started, and minutes later no report existed and no sweep process did either.
//
//      The passes that fit in one call DO work and are worth running — `focus,motion,type` reported 1136 rows and zero
//      failures against the operator's own panel. For `pages`, either give the detached child a console it can write to and
//      start it where the call is not the parent (a scheduled task, the way the relay was made durable), or measure those
//      axes from the harness in CI, which is where they already run.
//
//      One more, mine: `stdio: 'ignore'` on the detached attempt meant I could not read WHY it died. A background process you
//      cannot hear is a background process you cannot debug.
import { PROBE_SOURCE, failures, unmeasurable } from "./lib/contrast-probe.mjs";
// THE SAME MARK AXIS THE SWEEPS RUN, not a second copy of it (round 30 of the standing goal). The sweeps measure the
// HARNESS; this measures the panel the device actually serves, and the harness has no surface for several states the
// live one renders (the approval gate's disarmed ring is one — it measured 2.56 here and nothing else could see it).
import { marksSource } from "./lib/design-sweep.mjs";

const CONFIG_PATHS = [
  "D:\\\\Vale\\\\etc\\\\config.yaml", // layout-v2 (where it is today)
  "C:\\\\ProgramData\\\\Vale\\\\config.yaml",
  "C:\\\\ProgramData\\\\Vale\\\\etc\\\\config.yaml",
];

function emitted() {
  const script = `const fs = require('fs');
const PROBE = ${JSON.stringify(PROBE_SOURCE)};
const MARKS = ${JSON.stringify(marksSource("#root"))};
const CONFIGS = ${JSON.stringify(CONFIG_PATHS)};
(async () => {
  let token = null, where = null;
  for (const c of CONFIGS) {
    if (!fs.existsSync(c)) continue;
    const m = /token\\s*:\\s*["']?([A-Za-z0-9_\\-]{8,})/.exec(fs.readFileSync(c, 'utf8'));
    if (m) { token = m[1]; where = c; break; }
  }
  if (!token) {
    console.log(JSON.stringify({ error: 'no token found in any known config', tried: CONFIGS }));
    process.exit(1);
  }
  const { acquireBrowser } = require(process.env.VALE_BROWSER_HELPER);
  const { page, close } = await acquireBrowser();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e.message).slice(0, 120)));
  const out = { configAt: where, densities: {} };
  for (const [density, path_, vp] of [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]]) {
    await page.setViewportSize(vp);
    await page.goto('http://127.0.0.1:18080' + path_ + '?token=' + token, { waitUntil: 'load' });
    await page.waitForTimeout(3500);
    const state = await page.evaluate(() => ({
      connForm: !!document.getElementById('conn-form'),
      rootNodes: document.querySelectorAll('#root *').length,
      tabs: document.querySelectorAll('.tab, .dtab').length,
      railDots: [...document.querySelectorAll('.rail-dot, .dtab-dot')].map((d) => d.getAttribute('data-state')).slice(0, 8),
    }));
    const marks = await page.evaluate(MARKS);
    const rows = await page.evaluate(PROBE);
    const text = rows.filter((r) => r.kind !== 'graphic');
    const graphics = rows.filter((r) => r.kind === 'graphic');
    out.densities[density] = {
      state,
      rows: rows.length,
      text: text.length,
      graphics: graphics.length,
      textFailing: text.filter((r) => !r.inactive && r.cr < r.need).map((r) => r.sel + ' ' + r.cr + '<' + r.need),
      graphicFailing: graphics.filter((r) => !r.inactive && r.cr < r.need).map((r) => r.sel + ' ' + r.cr + '<' + r.need),
      unmeasurable: rows.filter((r) => r.cr === null || r.cr === undefined).length,
      // WHAT THE LIVE PANEL PAINTS, family by family, and any two states of one family that paint IDENTICALLY. The
      // harness reports the same shape; the value here is that these are the states the DEVICE renders, which is not
      // the same set — the sweeps' mark-coverage note counts six families the harness never paints.
      marks,
    };
  }
  out.errors = errors.slice(0, 4);
  // A READING BECOMES A GUARD (round 30 of the standing goal). Everything this probe collects is a list that must be
  // EMPTY on a healthy panel: failing text, failing graphics, and the two mark-axis verdicts the sweeps also make —
  // two states of one family painting identically, and a mark that is both a fill and a ring. Printing them and
  // exiting 0 would make the probe a report nobody has to answer; the exit code is what the runbook step can act on.
  // Note what a non-zero exit MEANS here: the live panel's state varies with what the device is doing (two sessions
  // today, fourteen another day), so it says "look at this", not "the build is broken".
  const bad = [];
  for (const [density, d] of Object.entries(out.densities)) {
    for (const key of ['textFailing', 'graphicFailing']) {
      for (const f of d[key]) bad.push(density + ' ' + key + ': ' + f);
    }
    if (d.marks && d.marks.collisions) for (const c of d.marks.collisions) bad.push(density + ' mark collision: ' + c);
    if (d.marks && d.marks.ringFill) for (const c of d.marks.ringFill) bad.push(density + ' ring+fill: ' + c);
  }
  out.verdict = bad.length ? { ok: false, problems: bad } : { ok: true };
  console.log(JSON.stringify(out, null, 1));
  await close();
  process.exit(bad.length ? 1 : 0);
})().catch((e) => { console.error('FATAL', e.message); process.exit(1); });`;

  // THE EMITTED SCRIPT MUST PARSE — the last of the five emitters in this repository to get the check the
  // panel has had since round 155. Five emit a standalone script for the device, and four were guarded:
  // this one, written in round 167, was not. A backtick in a comment here would have produced a broken
  // script for a run to fail on later, in the middle of 16 KB.
  try {
    new Function(script);
  } catch (e) {
    throw new Error("the emitted script does not parse: " + e.message);
  }
  return script;
}

if (process.argv.includes("--emit")) {
  process.stdout.write(emitted());
} else {
  console.log("usage: live-panel-probe.mjs --emit   # writes the device-side script to stdout");
  console.log("");
  console.log("Then, on the device: run it with the bundled Playwright runtime (browser_run_script).");
  console.log("Round 30 baseline (d1, 1.2.438, two sessions): panel 3 mark families [mark[working], sc-dot[ai],");
  console.log("  ag-dot[off]] · 0 collisions · 0 failing rows; desktop the same. ROUND 167's, for comparison:");
  console.log("  panel    1488 root nodes · 14 tabs · 32 rows (29 text, 3 graphics) · 0 failing");
  console.log("  harness   366 root nodes ·  3 tabs · 51 rows (42 text, 9 graphics) · 0 failing");
}
