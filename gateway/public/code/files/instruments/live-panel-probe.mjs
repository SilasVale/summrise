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
