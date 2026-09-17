#!/usr/bin/env node
// console-design-sweep — the gateway console's design, measured like the panel's.
//
// The console is a second UI behind Cloudflare Access, which is why nothing rendered it until round
// 55. Measuring it needs no login: build it to a temp dir, ship that to the device, and serve it
// from disk through Playwright routes with the same /api fixtures its own render smokes use.
//
//   cd gateway/ui && npx vite build --outDir /tmp/console-build --emptyOutDir
//   tar czf /tmp/console.tgz -C /tmp/console-build .   (ship it; extract on the device to
//                                                       C:\ProgramData\Vale\pwout\console)
//   node agent/scripts/console-design-sweep.mjs --emit > /tmp/console-sweep.js   (run on the device)
//   node agent/scripts/console-design-sweep.mjs --judge <report.json>
//
// The checks and the judge are the shared core's; this file supplies the URL, the page list, the API
// fixtures, the login pass (which exists only when /api/me answers 401) and the three widths.
//
// AXES COVERED, so nobody re-measures what is already known (round 85):
//   contrast (resting) · contrast (HOVERED — the 24 :hover rules in the console's sheet) · geometry,
//   clipping and slivers at 1440/900/720 · WCAG reflow · accessible names · keyboard focus rings.
//   Hover measured clean on 2026-09-16: 6 pages, 128 interactive elements (13-38 each), 0 under AA —
//   unlike the panel's, where the first hover run found a dark-theme button at 1.94 (round 84).
// NOT covered: the Electron-only shell, and any state the fixtures cannot produce (the login pass is
// the only 401 path here).
//
// MEASURED BY HAND, NOT YET BY THIS TOOL (round 88): every class on screen, checked against the
// browser's own parsed selectors. 221 classes are styled; three were not —
//   ov-firstrun, ov-firstrun-line   class names in Overview.tsx that NO stylesheet ever defined
//                                   (they arrived with the first-run card and were never styled);
//                                   pruned from the markup, where they had never done anything.
//   stat-off                        the Overview's default tone, and DELIBERATELY unstyled:
//                                   .stat-card::before already paints the faint bar that "off" means.
// The pass was written into this file and then REMOVED: its emitted form split class names on the
// letter "s" when it reached the device, so its report listed "btn btn-" and "rail-clu" as unstyled.
// The browser gave the right answer when asked by hand; a check whose output I could not trust does
// not ship. Whoever picks this up should report the STYLED count beside the unstyled list, so an
// empty read can never look like a clean page.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { failures, unmeasurable, PROBE_SOURCE } from "./lib/contrast-probe.mjs";
import { pageChecks, judgeReport, reportSummary, UNSTYLED_SOURCE, focusPass, motionPass, TARGETS_SOURCE, THEME_SOURCE } from "./lib/design-sweep.mjs";

const mode = process.argv[2];

function browserScript() {
  const script = `const fs = require('fs');
const path = require('path');
const ROOT = 'C:\\\\ProgramData\\\\Vale\\\\pwout\\\\console';
const PROBE = ${JSON.stringify(PROBE_SOURCE)};
const UNSTYLED = ${JSON.stringify(UNSTYLED_SOURCE)};
const focusPass = ${focusPass.toString()};
const TARGETS = ${JSON.stringify(TARGETS_SOURCE)};
const THEME = ${JSON.stringify(THEME_SOURCE)};
const motionPass = ${motionPass.toString()};
${pageChecks("#root")}
const now = Date.now();
// The console's own render-smoke fixtures (gateway/ui/*-render-smoke.mjs), so the browser renders the
// same pages those tests assert against in jsdom — same data, real layout, real colours.
const API = {
  '/api/me': { username: 'admin', role: 'admin', token: 'tok-abc', keys: { DEEPSEEK_API_KEY: { configured: true, masked: 'sk-1' } } },
  '/api/me/keys': { keys: [
    { name: 'DEEPSEEK_API_KEY', configured: true, masked: 'sk-1', usage: 12 },
    { name: 'OPENAI_API_KEY', configured: false, masked: '', usage: 0 },
  ] },
  '/api/me/route': { effective: 'og/deepseek/deepseek-v4.1-flash' },
  '/api/me/usproxy': { enabled: false },
  '/api/status': { ok: true, version: '1.0.106' },
  '/api/version': { version: '1.0.106' },
  '/api/devices': { devices: [
    { name: 'd1', hostname: 'd1.agent.saisi.online', token: 'a1b2c3d4e5f6g7h8', registeredAt: now - 86400000, lastSeenAt: now - 60000, lastVersion: '1.0.106' },
    { name: 'd2', hostname: 'd2.agent.saisi.online', token: 'z9y8x7w6v5u4t3s2', lastVersion: '1.0.100' },
  ] },
  '/api/devices/install-cmd': { ok: true, version: '1.0.106', download: 'https://v.saisi.online/vale-agent-latest.tgz' },
  '/api/devices/register-keys': { keys: [{ code: 'abcd1234', expiresAt: now + 3600000 }] },
  '/api/plugins/status': { devices: { d1: { online: false, agent_up: true, tunnel_up: true, version: '1.0.106', checked_at: now, verdict: 'crashed' } } },
  // ALL FOUR CHANNELS, because the lane rules are per-channel: with only 'og' in the fixture the
  // three fills that do NOT flip with the theme never render, and a contrast fix for them could not
  // be seen. Measured round 79 — the ink/fill pairing differs per lane on purpose.
  '/api/health': { channels: [
    { prefix: 'og/', ok: true },
    { prefix: 'ds/', ok: true },
    { prefix: 'or/', ok: false },
    { prefix: 'qw/', ok: true },
  ] },
  '/api/admin/providers': { providers: [{ prefix: 'my/', label: 'My Provider', baseURL: 'https://api.example.com', api: 'openai-completions', models: [{ id: 'llama-3' }], advertised: ['my/llama-3'], keyEnv: '', keyMasked: 'sk-9876', keyReady: true }], apis: [], filePrefixes: [] },
  '/api/admin/models': { models: [{ id: 'my/llama-3', label: 'llama-3' }] },
  '/api/admin/catalogue': { models: [{ id: 'my/llama-3', label: 'llama-3' }] },
  '/api/admin/users': { users: [{ username: 'operator', role: 'admin', createdAt: now - 86400000 }, { username: 'guest', role: 'user', createdAt: now - 3600000 }] },
  '/api/admin/public': { enabled: false },
};
// The console's own route table (gateway/ui/src/App.tsx) — every authenticated page it has.
const PAGES = [
  ['overview', '#/'],
  ['devices', '#/devices'],
  ['models', '#/models'],
  ['keys', '#/keys'],
  ['routes', '#/routes'],
  ['users', '#/users'],
];
const auth = { signedIn: true };
// AN EMPTY FLEET, which the console has never been measured in: the fixture table carries two devices and
// two keys, so every surface has always been rendered with content. A console with nothing registered is a
// real state (a fresh deployment) and the one most likely to have an undesigned blank pane. The route
// handler consults this, and every other fixture is untouched so the two passes differ in exactly one way.
const empty = { fleet: false };
(async () => {
  const { acquireBrowser } = require(process.env.VALE_BROWSER_HELPER);
  const { page, close } = await acquireBrowser();
  await page.route('https://ai.saisi.online/**', (route) => {
    const p = new URL(route.request().url()).pathname;
    // The login page exists only when /api/me answers 401, so it gets its own pass with the flag
    // flipped — not a fixture.
    if (auth.signedIn === false && p === '/api/me') {
      return route.fulfill({ status: 401, contentType: 'application/json', headers: { 'cache-control': 'no-store' }, body: JSON.stringify({ type: 'error', error: { message: 'unauthorized' } }) });
    }
    if (p.startsWith('/api/')) {
      let body = API[p] === undefined ? {} : API[p];
      if (empty.fleet && p === '/api/devices') body = { devices: [] };
      if (empty.fleet && p === '/api/me/keys') body = { keys: [] };
      return route.fulfill({ status: 200, contentType: 'application/json', headers: { 'cache-control': 'no-store' }, body: JSON.stringify(body) });
    }
    const file = p === '/' || p === '' ? 'index.html' : p.replace(/^\\//, '');
    const full = path.join(ROOT, file);
    const body = fs.existsSync(full) ? fs.readFileSync(full) : fs.readFileSync(path.join(ROOT, 'index.html'));
    const ext = path.extname(full);
    const type = ext === '.js' ? 'text/javascript' : ext === '.css' ? 'text/css' : ext === '.svg' ? 'image/svg+xml' : 'text/html; charset=utf-8';
    return route.fulfill({ status: 200, contentType: type, headers: { 'cache-control': 'no-store' }, body });
  });
  const report = { rows: [], surfaces: [], names: [], focus: [], reflow: [], hover: [], unstyled: [], motion: [], targets: [], themeChecks: [] };
  // THE CONSOLE IN DARK. It has a dark theme — body[data-theme=dark], applied before the first paint and
  // persisted in localStorage — and every section of this sweep hardcoded theme: 'light', so a dark
  // regression has been invisible here for as long as the sweep has existed. Round 175 found the same gap in
  // the panel's fixture surfaces; this is the second home, and it is being done before it costs anything.
  // ONE WIDTH, not all three: 1440 is where the console is used, and three widths of dark would double a run
  // that already takes minutes for a difference that width does not create.
  for (const [label, hash] of PAGES) {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('https://ai.saisi.online/?cb=' + Date.now(), { waitUntil: 'load' });
    await page.evaluate((h) => {
      try { localStorage.setItem('vale-theme', 'dark'); } catch (e) {}
      document.body.setAttribute('data-theme', 'dark');
      location.hash = h;
    }, hash);
    await page.waitForTimeout(1600);
    // READ IT OFF THE PAGE rather than trusting the instruction (round 176's lesson).
    report.themeChecks.push({ page: label + '-dark', intended: 'dark', ...(await page.evaluate(THEME)) });
    const rows = await page.evaluate(PROBE);
    for (const r of rows) report.rows.push({ ...r, page: label + '-dark', width: 1440, density: 'console', theme: 'dark' });
    report.surfaces.push({ page: label + '-dark', width: 1440, ...(await page.evaluate(SURFACE)) });
    report.names.push({ page: label + '-dark', ...(await page.evaluate(NAMES)) });
  }
  await page.evaluate(() => { try { localStorage.setItem('vale-theme', 'light'); } catch (e) {} document.body.setAttribute('data-theme', 'light'); });

  for (const width of [1440, 900, 720]) {
    await page.setViewportSize({ width, height: 900 });
    for (const [label, hash] of PAGES) {
      await page.goto('https://ai.saisi.online/?cb=' + Date.now(), { waitUntil: 'load' });
      await page.evaluate((h) => { location.hash = h; }, hash);
      await page.waitForTimeout(1600);
      const rows = await page.evaluate(PROBE);
      for (const r of rows) report.rows.push({ ...r, page: label, width, density: 'console', theme: 'light' });
      report.surfaces.push({ page: label, width, ...(await page.evaluate(SURFACE)) });
      if (width === 1440) {
        report.names.push({ page: label, ...(await page.evaluate(NAMES)) });
        // Rendered classes with no matching rule — the mirror of dead CSS, and the failure a prune
        // causes. The browser's parsed selectors are the authority (rounds 79-80 removed 300+ lines
        // from this sheet). The styled count travels with the list as the tripwire. (No backticks in
        // here: this text is inside the emitted template, and the ninth stray one shut --emit down.)
        report.unstyled.push({ page: label, ...(await page.evaluate(UNSTYLED)) });
        // HOVER, the state round 84 added for the panel — where its first run found a dark-theme
        // button at 1.94. The console has its own 24 :hover rules and a different token set, and had
        // never been measured hovering. One element per control family, at the widest viewport only,
        // because hover styles are per-class and the cost is a full-DOM probe per hover.
        {
          const all = await page.$$('button, [role="button"], a');
          const seenClass = new Set();
          const underAA = [];
          for (const h of all) {
            const key = await h.evaluate((el) => (typeof el.className === 'string' ? el.className : el.tagName));
            if (seenClass.has(key)) continue;
            seenClass.add(key);
            const box = await h.boundingBox();
            if (!box || box.width < 2 || box.height < 2) continue;
            try {
              await h.hover({ timeout: 400 });
            } catch (e) {
              continue;
            }
            await page.waitForTimeout(90);
            for (const r of await page.evaluate(PROBE)) {
              const need = r.need ?? 4.5;
              if (r.cr !== null && !r.inactive && r.cr < need) {
                underAA.push(r.sel + ' "' + String(r.text).slice(0, 16) + '" ' + r.cr + '<' + need);
              }
            }
            await page.mouse.move(2, 2);
          }
          report.hover.push({ page: label, width, density: 'console', theme: 'light', interactive: all.length, underAA: [...new Set(underAA)] });
        }
        await page.evaluate(() => document.body.focus());
        // ONE implementation, shared with the panel and the extension (lib/design-sweep.mjs).
        report.focus.push(await focusPass(page, 16, { page: label, width }));
      }
    }
  }
  // REDUCED MOTION, both states, on the console's own overview page. The panel has had this since round
  // 134 and the console had nothing: the gap was recorded in round 136 when the pass was shared and left
  // unwired. render re-loads the page and re-applies the route, because the preference only takes
  // effect on a fresh style resolution.
  for (const width of [1440]) {
    await page.setViewportSize({ width, height: 900 });
    const render = async () => {
      await page.goto('https://ai.saisi.online/?cb=' + Date.now(), { waitUntil: 'load' });
      await page.evaluate((h) => { location.hash = h; }, '#/');
      await page.waitForTimeout(1400);
    };
    report.motion.push(await motionPass(page, render, { page: 'overview', width, density: 'console', theme: 'light' }));
  }

  // TARGET SIZE, WCAG 2.5.8 — the check round 162 added for the PANEL, wired here because a check that
  // exists in one UI and not the others is the pattern this suite keeps paying for (rounds 135-136, 141).
  for (const [label, hash] of [['overview', '#/'], ['devices', '#/devices']]) {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('https://ai.saisi.online/?cb=' + Date.now(), { waitUntil: 'load' });
    await page.evaluate((h) => { location.hash = h; }, hash);
    await page.waitForTimeout(1500);
    report.targets.push({ page: label, ...(await page.evaluate(TARGETS)) });
  }

  // THE EMPTY FLEET, as surfaces of its own. Two pages have a meaningful empty form — Devices and Keys —
  // and neither had ever been rendered without content. Same recipe as the panel's empty state: a top-level
  // pass, one render each, recorded like any other surface.
  {
    empty.fleet = true;
    for (const [label, hash] of [['devices-empty', '#/devices'], ['keys-empty', '#/keys']]) {
      await page.setViewportSize({ width: 1440, height: 900 });
      await page.goto('https://ai.saisi.online/?cb=' + Date.now(), { waitUntil: 'load' });
      await page.evaluate((h) => { location.hash = h; }, hash);
      await page.waitForTimeout(1600);
      const rows = await page.evaluate(PROBE);
      for (const r of rows) report.rows.push({ ...r, page: label, width: 1440, density: 'console', theme: 'light' });
      report.surfaces.push({ page: label, width: 1440, ...(await page.evaluate(SURFACE)) });
      report.names.push({ page: label, ...(await page.evaluate(NAMES)) });
    }
    empty.fleet = false;
  }

  auth.signedIn = false;
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('https://ai.saisi.online/?cb=' + Date.now(), { waitUntil: 'load' });
  await page.waitForTimeout(1600);
  for (const r of await page.evaluate(PROBE)) report.rows.push({ ...r, page: 'login', width: 1440, density: 'console', theme: 'light' });
  report.surfaces.push({ page: 'login', width: 1440, ...(await page.evaluate(SURFACE)) });
  report.names.push({ page: 'login', ...(await page.evaluate(NAMES)) });
  fs.writeFileSync('C:\\\\ProgramData\\\\Vale\\\\pwout\\\\console-sweep.json', JSON.stringify(report));
  console.log(JSON.stringify({ rows: report.rows.length, surfaces: report.surfaces.length }));
  await close();
})().catch((e) => { console.error('FATAL', e.message); process.exit(1); });`;

  // THE EMITTED SCRIPT MUST PARSE — the check the panel emitter has had since round 155 and this one
  // never did. Its absence cost two node --check cycles on backticks inside a COMMENT here: a backtick
  // ends the template early and the emitted 30 KB script stops parsing somewhere in the middle, which is
  // exactly the failure this asks about. The sweep gate compiles the emitted script for the panel; this
  // makes the console emitter refuse to produce a broken one in the first place.
  try {
    new Function(script);
  } catch (e) {
    throw new Error("the emitted script does not parse: " + e.message);
  }
  return script;
}

function judge(file) {
  const report = JSON.parse(readFileSync(file, "utf8"));
  const findings = judgeReport(report, {
    navless: ["login"],
    implicitStates: {
      "stat-off": "the Overview's default tone: the base .stat-card::before already paints the faint bar that off means",
    },
  });
  for (const t of report.themeChecks || []) {
    const seen = t.stored || t.attr;
    if (seen && seen !== t.intended) {
      findings.push(`theme: ${t.page} was navigated as "${t.intended}" and rendered "${seen}" — the report would be describing a page it did not render`);
    }
  }
  for (const t of report.targets || []) {
    for (const u of t.distinct || []) {
      if (!u.passesBySpacing) {
        findings.push(`target size (${t.page}): ${u.sel} is ${u.w}x${u.h} with its nearest neighbour ${u.nearest}px away — 2.5.8 wants 24x24 or 24px of spacing ("${u.text}")`);
      }
    }
  }
  for (const r of failures(report.rows).slice(0, 10)) {
    findings.unshift(`${r.cr} ${r.page}${r.width ? "@" + r.width + "px" : ""} ${r.sel} "${String(r.text).slice(0, 24)}"`);
  }
  console.log(reportSummary("console", report));
  if (unmeasurable(report.rows).length) console.log(`note: ${unmeasurable(report.rows).length} node(s) unmeasurable`);
  if (!findings.length) {
    console.log("console design sweep OK: nothing above found a defect");
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
    console.error("usage: console-design-sweep.mjs --judge <report.json>");
    process.exit(2);
  }
  process.exit(judge(file));
} else {
  console.error(readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n").slice(1, 18).join("\n"));
  console.error("\nusage: console-design-sweep.mjs --emit | --judge <report.json>");
  process.exit(2);
}
