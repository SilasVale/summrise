#!/usr/bin/env node
// extension-design-sweep — the browser extension's options page, measured like everything else.
//
// The extension is the last UI without a sweep. Its options page is plain HTML/CSS/JS that talks to
// chrome.storage.local, so it renders for real over a test origin with a storage shim injected — and
// its first render (round 58, done by hand) found TWO h1s and an inline style. This file makes that
// measurement repeatable instead of re-improvised, and it is built on the same core as the panel's
// and the console's sweeps (`lib/design-sweep.mjs`), so all three apply the same rules.
//
//   node agent/scripts/extension-design-sweep.mjs --emit > /tmp/ext-sweep.js
//   node agent/scripts/extension-design-sweep.mjs --judge <report.json>
//
// The extension files must be on the device (they are tiny): tar `extension/{options,shared.js,
// manifest.json}` and extract to C:\ProgramData\Vale\pwout\extension.
//
// WHAT IT CANNOT SEE: the content script's behaviour on a real code-server page (its pure guards are
// unit-tested in extension/test/), and anything the chrome APIs do for real — the shim answers with
// fixed values.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { failures, unmeasurable, PROBE_SOURCE, contrastRatio, parseColour } from "./lib/contrast-probe.mjs";
import { pageChecks, judgeReport, reportSummary, focusPass, UNSTYLED_SOURCE, TARGETS_SOURCE, DIAG_SOURCE } from "./lib/design-sweep.mjs";

const mode = process.argv[2];

function browserScript() {
  const script = `const fs = require('fs');
const path = require('path');
const ROOT = 'C:\\\\ProgramData\\\\Vale\\\\pwout\\\\extension';

// THE ENTRY THIS SWEEP WAS EMITTED AGAINST. Both UIs are measured from a DELIVERED copy of their
// build, and nothing said which generation it was: round 184 found the console's directory holding eight
// files from four generations, and round 189 lost an afternoon to a stale PANEL harness whose collapsed
// tab strip read as a live regression. The panel's harness now stamps itself; these two carry the entry's
// digest instead, because a stale delivery always shows up in the file that names everything else.
const EXPECTED_ENTRY = {"bytes": 3596, "sha": "0647992fe70c"}\;
const EXPECTED_ENTRY_PATH = "C:\\\\ProgramData\\\\Vale\\\\pwout\\\\extension\\\\options\\\\options.html";
const PROBE = ${JSON.stringify(PROBE_SOURCE)};
${pageChecks("body")}
const focusPass = ${focusPass.toString()};
const UNSTYLED = ${JSON.stringify(UNSTYLED_SOURCE)};
const TARGETS = ${JSON.stringify(TARGETS_SOURCE)};
// THE SWEEP REPORTS ITSELF TO THE AGENT'S DIAGNOSTIC RING (round 196), so a caller whose tool call timed out
// can tell a run that is still working from one that was killed. Same helper as the other two adapters.
${DIAG_SOURCE}
// The page's only chrome API. Fixed values: the sweep measures the PAGE, not the storage layer.
// storage.empty is flipped by the sweep: a fresh install has NO stored values, so the page must
// fall back to its defaults (DEFAULT_STUDIO_ORIGIN, links off) rather than rendering blanks.
const storage = { empty: false };
const shim = () => "<script>window.chrome={storage:{local:{get:(k,cb)=>{const v=storage.empty?{}:{studioOrigin:'https://vscode.saisi.online',studioLinksEnabled:true};if(cb)cb(v);return Promise.resolve(v);},set:(v,cb)=>{if(cb)cb();return Promise.resolve();}}}};</script>";

(async () => {
  const { acquireBrowser } = require(process.env.VALE_BROWSER_HELPER);
  const { page, close } = await acquireBrowser();
  await page.route('http://vale-ext.test/**', (route) => {
    const p = new URL(route.request().url()).pathname.replace(/^\\//, '');
    const full = path.join(ROOT, p || 'options/options.html');
    if (!fs.existsSync(full)) return route.fulfill({ status: 404, body: 'not found' });
    const ext = path.extname(full);
    let body = fs.readFileSync(full);
    if (ext === '.html') body = Buffer.from(fs.readFileSync(full, 'utf8').replace('<head>', '<head>' + shim()));
    const type = ext === '.js' ? 'text/javascript' : ext === '.css' ? 'text/css' : 'text/html; charset=utf-8';
    return route.fulfill({ status: 200, contentType: type, headers: { 'cache-control': 'no-store' }, body });
  });
  await diag("start extension pid=" + process.pid);
  const report = { rows: [], surfaces: [], names: [], focus: [], unstyled: [], targets: [] , entryCheck: (() => { try { const b = fs.readFileSync(EXPECTED_ENTRY_PATH); const c = require("crypto").createHash("sha256").update(b).digest("hex").slice(0, 12); return { bytes: b.length, sha: c, expected: EXPECTED_ENTRY, stale: b.length !== EXPECTED_ENTRY.bytes || c !== EXPECTED_ENTRY.sha }; } catch (e) { return { error: String(e.message).slice(0, 60), expected: EXPECTED_ENTRY, stale: true }; } })() };
  // TWO STORAGE STATES. Empty storage is the state a NEW INSTALL is in — the origin falls back to
  // DEFAULT_STUDIO_ORIGIN and the links toggle starts off — and it is a different page to look at
  // than the configured one. Measured by hand in round 63; repeated here so it stays measured.
  for (const empty of [false, true]) {
    storage.empty = empty;
    for (const width of [900, 600, 360]) {
      await page.setViewportSize({ width, height: 800 });
      await page.goto('http://vale-ext.test/options/options.html?cb=' + Date.now(), { waitUntil: 'load' });
      await page.waitForTimeout(800);
      const rows = await page.evaluate(PROBE);
      for (const r of rows) report.rows.push({ ...r, page: empty ? 'options-fresh' : 'options', width, density: 'extension', theme: 'light' });
      report.surfaces.push({ page: empty ? 'options-fresh' : 'options', width, ...(await page.evaluate(SURFACE)) });
      if (width === 900) report.names.push({ page: empty ? 'options-fresh' : 'options', ...(await page.evaluate(NAMES)) });
      // THE THIRD UI FOCUS RINGS. report.focus was declared here and never filled, so the extension
      // had no focus measurement at all — while the panel and the console both had one (and had drifted
      // from each other). One shared implementation now.
      report.focus.push(await focusPass(page, 14, { page: empty ? 'options-fresh' : 'options', width }));
      // UNSTYLED CLASSES — the mirror of dead CSS, and the failure a prune causes. The third UI had no
      // such check while the panel and console both did; same shared collector, embedded the same way.
      report.unstyled.push({ page: empty ? 'options-fresh' : 'options', width, ...(await page.evaluate(UNSTYLED)) });
      // TARGET SIZE, WCAG 2.5.8 — the third home for the check round 162 added, so all three UIs are covered.
      if (width === 900) report.targets.push({ page: empty ? 'options-fresh' : 'options', ...(await page.evaluate(TARGETS)) });
    }
  }
  // THE TWO TRANSIENT MESSAGE TONES, MEASURED ON EVERY RUN. Round 149 found that the refusal and the
  // confirmation were painted identically because #status had no rule at all, and fixed it with
  // data-state="error" / "ok". That fix was measured ONCE and swept by nothing — every sweep renders the
  // page at rest, and #status is empty until you press Save. Same rule as rounds 151-152: a one-off
  // measurement is not a guard. Both states are driven here with the page's own controls, and the colour
  // is compared with the probe's maths rather than by eye.
  const messageRows = [];
  for (const [label, origin] of [['refusal', 'not a url at all'], ['saved', 'https://studio.example']]) {
    await page.setViewportSize({ width: 900, height: 800 });
    await page.goto('http://vale-ext.test/options/options.html?cb=' + Date.now(), { waitUntil: 'load' });
    await page.waitForTimeout(500);
    await page.fill('#studioOrigin', origin);
    await page.click('#save');
    await page.waitForTimeout(250);
    const seen = await page.evaluate(() => {
      const el = document.getElementById('status');
      const cs = getComputedStyle(el);
      const host = el.parentElement ? getComputedStyle(el.parentElement) : null;
      return {
        state: el.dataset.state || '',
        text: el.textContent.slice(0, 30),
        color: cs.color,
        size: parseFloat(cs.fontSize),
        // A TRANSPARENT SURFACE IS NOT A SURFACE, and it is a truthy string: rgba(0, 0, 0, 0) sailed past
        // the || and would have been judged as transparent BLACK. Measured on the device, caught before it
        // produced a number.
        surface: (() => {
          const own = host ? host.backgroundColor : '';
          const opaque = (c) => c && !/rgba\(0, 0, 0, 0\)/.test(c);
          if (opaque(own)) return own;
          let el = document.getElementById('status').parentElement;
          while (el) {
            const bg = getComputedStyle(el).backgroundColor;
            if (opaque(bg)) return bg;
            el = el.parentElement;
          }
          return getComputedStyle(document.body).backgroundColor;
        })(),
        // AND THE PAGE'S OWN BACKGROUND, recorded separately. The walk above has returned a transparent
        // value in every run so far despite the body reporting rgb(245, 245, 247) when asked directly, and
        // rather than spend a fourth edit on why, both values are recorded and the JUDGE picks: it is the
        // side that owns the maths, and it can prefer an opaque value without guessing which one is right.
        bodyBg: getComputedStyle(document.body).backgroundColor,
      };
    });
    messageRows.push({ label, ...seen });
  }
  // RAW VALUES ONLY. The maths lives in lib/contrast-probe.mjs and the emitted script runs on the DEVICE,
  // where an imported function does not exist — the judge below computes the ratios, exactly as it does for
  // every contrast row. (First version of this pass called contrastRatio here and died with
  // "contrastRatio is not defined" on the device.)
  report.messages = messageRows;

  await diag("done rows=" + (report.rows || []).length + " findings-source-ready pid=" + process.pid);
  fs.writeFileSync('C:\\\\ProgramData\\\\Vale\\\\pwout\\\\ext-sweep.json', JSON.stringify(report));
  console.log(JSON.stringify({ rows: report.rows.length, surfaces: report.surfaces.length }));
  await close();
})().catch((e) => { console.error('FATAL', e.message); process.exit(1); });`;

  // THE EMITTED SCRIPT MUST PARSE — the third home for a check the panel emitter has had since round 155
  // and the console got in round 177. This one had none, so a backtick in a comment here would have
  // produced a broken 30 KB script for a run to fail on later, with the failure landing somewhere in the
  // middle of it. The same guard, written the same way, so all three emitters refuse alike.
  try {
    new Function(script);
  } catch (e) {
    throw new Error("the emitted script does not parse: " + e.message);
  }
  return script;
}

function judge(file) {
  const report = JSON.parse(readFileSync(file, "utf8"));
  // The options page has no navigation in EITHER storage state, so both page names are listed —
  // the first version named only "options" and the judge failed the fresh-install pass, which is
  // exactly what an exemption that drifts out of step should do.
  // unstyledFloor: 2 — this page is THREE controls styled by element and id selectors; its sheet defines
  // four classes, so the judge's 100 floor would report a false alarm on every run. Two means the
  // collector read at least something real, which is what the floor is for.
  const findings = judgeReport(report, { navless: ["options", "options-fresh"], unstyledFloor: 2 });
  for (const r of failures(report.rows).slice(0, 10)) {
    findings.unshift(`${r.cr} ${r.page}@${r.width}px ${r.sel} "${String(r.text).slice(0, 24)}"`);
  }
  // THE MESSAGE STATES ARE JUDGED, not merely recorded — and on BOTH counts. The contrast is the easy
  // half; the defect round 149 actually fixed was that the refusal and the confirmation were painted the
  // same colour, so the UI could not tell you whether your change was stored. A guard that only checks
  // contrast would pass a regression straight back to that state.
  for (const t of report.targets || []) {
    for (const u of t.distinct || []) {
      if (!u.passesBySpacing) {
        findings.push(`target size (${t.page}): ${u.sel} is ${u.w}x${u.h} with its nearest neighbour ${u.nearest}px away — 2.5.8 wants 24x24 or 24px of spacing ("${u.text}")`);
      }
    }
  }
  const MSG_OPAQUE = (c) => !!c && !/rgba\(0, 0, 0, 0\)/.test(c);
  const msgs = (report.messages || []).map((m) => {
    const need = m.size >= 18 ? 3 : 4.5;
    // Prefer whichever surface is actually opaque; the page records both because a transparent string is
    // truthy and would otherwise be judged as transparent black.
    const surface = MSG_OPAQUE(m.surface) ? m.surface : MSG_OPAQUE(m.bodyBg) ? m.bodyBg : m.surface;
    let cr = null;
    try {
      cr = Number(contrastRatio(parseColour(m.color), parseColour(surface)).toFixed(2));
    } catch {
      cr = null;
    }
    return { ...m, surface, need, cr };
  });
  for (const m of msgs) {
    if (m.cr === null) findings.unshift(`message ${m.label}: unmeasurable (${m.color} on ${m.surface})`);
    else if (m.cr < m.need) findings.unshift(`message ${m.label}: contrast ${m.cr} < ${m.need} ("${m.text}")`);
    if (!m.state) findings.unshift(`message ${m.label}: no data-state, so no tone is applied ("${m.text}")`);
  }
  if (msgs.length === 2 && msgs[0].color === msgs[1].color) {
    findings.unshift(`the refusal and the confirmation are painted the same colour (${msgs[0].color}) — the round-149 defect, back`);
  }
  console.log(reportSummary("extension", report));
  if (unmeasurable(report.rows).length) console.log(`note: ${unmeasurable(report.rows).length} node(s) unmeasurable`);
  // SAID OUT LOUD ON EVERY RUN, because a measurement nobody sees is a measurement nobody trusts. These two
  // states are transient and would otherwise be invisible in a report full of resting-page rows.
  if (msgs.length) {
    console.log(`note: ${msgs.length} transient message state(s): ` + msgs.map((m) => `${m.label} ${m.state} ${m.cr} (needs ${m.need})`).join(' · '));
  }
  if (!findings.length) {
    console.log("extension design sweep OK: nothing above found a defect");
    return 0;
  }
  console.error(`\n${findings.length} finding(s):\n  ` + findings.join("\n  "));
  return 1;
}

if (mode === "--emit") {
  process.stdout.write(browserScript());
} else if (mode === "--judge") {
  const file = process.argv[3];
  if (!file) {
    console.error("usage: extension-design-sweep.mjs --judge <report.json>");
    process.exit(2);
  }
  process.exit(judge(file));
} else {
  console.error(readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n").slice(1, 16).join("\n"));
  console.error("\nusage: extension-design-sweep.mjs --emit | --judge <report.json>");
  process.exit(2);
}
