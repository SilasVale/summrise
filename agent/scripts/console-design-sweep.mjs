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
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { failures, unmeasurable, PROBE_SOURCE } from "./lib/contrast-probe.mjs";
import { pageChecks, judgeReport, reportSummary } from "./lib/design-sweep.mjs";

const mode = process.argv[2];

function browserScript() {
  return `const fs = require('fs');
const path = require('path');
const ROOT = 'C:\\\\ProgramData\\\\Vale\\\\pwout\\\\console';
const PROBE = ${JSON.stringify(PROBE_SOURCE)};
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
      const body = API[p] === undefined ? {} : API[p];
      return route.fulfill({ status: 200, contentType: 'application/json', headers: { 'cache-control': 'no-store' }, body: JSON.stringify(body) });
    }
    const file = p === '/' || p === '' ? 'index.html' : p.replace(/^\\//, '');
    const full = path.join(ROOT, file);
    const body = fs.existsSync(full) ? fs.readFileSync(full) : fs.readFileSync(path.join(ROOT, 'index.html'));
    const ext = path.extname(full);
    const type = ext === '.js' ? 'text/javascript' : ext === '.css' ? 'text/css' : ext === '.svg' ? 'image/svg+xml' : 'text/html; charset=utf-8';
    return route.fulfill({ status: 200, contentType: type, headers: { 'cache-control': 'no-store' }, body });
  });
  const report = { rows: [], surfaces: [], names: [], focus: [], reflow: [] };
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
        await page.evaluate(() => document.body.focus());
        let noRing = 0;
        for (let i = 0; i < 16; i++) {
          await page.keyboard.press('Tab');
          const ok = await page.evaluate(() => {
            const el = document.activeElement;
            if (!el || el === document.body) return true;
            const st = getComputedStyle(el);
            return (parseFloat(st.outlineWidth) > 0 && st.outlineStyle !== 'none') || (st.boxShadow && st.boxShadow !== 'none');
          });
          if (!ok) noRing++;
        }
        if (noRing) report.focus.push({ page: label, missing: noRing });
      }
    }
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
}

function judge(file) {
  const report = JSON.parse(readFileSync(file, "utf8"));
  const findings = judgeReport(report, { navless: ["login"] });
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
