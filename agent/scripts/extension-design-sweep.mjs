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
import { failures, unmeasurable, PROBE_SOURCE } from "./lib/contrast-probe.mjs";
import { pageChecks, judgeReport, reportSummary, focusPass, UNSTYLED_SOURCE } from "./lib/design-sweep.mjs";

const mode = process.argv[2];

function browserScript() {
  return `const fs = require('fs');
const path = require('path');
const ROOT = 'C:\\\\ProgramData\\\\Vale\\\\pwout\\\\extension';
const PROBE = ${JSON.stringify(PROBE_SOURCE)};
${pageChecks("body")}
const focusPass = ${focusPass.toString()};
const UNSTYLED = ${JSON.stringify(UNSTYLED_SOURCE)};
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
  const report = { rows: [], surfaces: [], names: [], focus: [], unstyled: [] };
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
    }
  }
  fs.writeFileSync('C:\\\\ProgramData\\\\Vale\\\\pwout\\\\ext-sweep.json', JSON.stringify(report));
  console.log(JSON.stringify({ rows: report.rows.length, surfaces: report.surfaces.length }));
  await close();
})().catch((e) => { console.error('FATAL', e.message); process.exit(1); });`;
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
  console.log(reportSummary("extension", report));
  if (unmeasurable(report.rows).length) console.log(`note: ${unmeasurable(report.rows).length} node(s) unmeasurable`);
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
