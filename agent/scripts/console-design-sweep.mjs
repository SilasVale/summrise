#!/usr/bin/env node
// console-design-sweep — the gateway console's design, measured the same way the panel's is.
//
// WHY THIS EXISTS. The console is a second UI with its own stylesheet and tokens, and it had never
// been through a design pass: the panel has had contrast, hierarchy, landmarks, focus, geometry,
// reflow, accessible-name and performance sweeps for many rounds, while the console sits behind
// Cloudflare Access — so nothing had ever rendered it in a browser with a probe attached. Round 55
// measured three of its pages and found the account avatar at 2.13 against its own gradient. This
// file is that measurement, packaged so it is not rewritten every time.
//
// THE SSO IS NOT AN OBSTACLE, because the console is a static bundle plus JSON:
//
//   1. build it OUTSIDE the repo (so the shipped assets are untouched):
//        cd gateway/ui && npx vite build --outDir /tmp/console-build --emptyOutDir
//   2. ship that directory to the device (the relay pair; ~400 KB, tar first), extract it to
//      C:\ProgramData\Vale\pwout\console
//   3. node agent/scripts/console-design-sweep.mjs --emit > /tmp/console-sweep.js
//      …hand that to the device's browser (console MCP browser_run_script), save the returned JSON
//   4. node agent/scripts/console-design-sweep.mjs --judge <report.json>
//
// The fixtures are the ones the console's own render smokes use (`gateway/ui/*-render-smoke.mjs`),
// so the browser renders the same pages those tests assert against in jsdom — same data, real
// layout, real colours.
//
// WHAT IT CANNOT SEE, stated rather than implied:
//   * anything that needs a REAL login or real data: the stubs are fixtures, so a page's empty state
//     is what gets measured when a stub is missing rather than the shape production would show;
//   * behaviour (a button that calls the wrong endpoint), which the console's node --test suites and
//     its render smokes cover;
//   * the console's dark theme, if it has one — the sweep measures the theme the browser loads.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { failures, unmeasurable, PROBE_SOURCE } from "./lib/contrast-probe.mjs";

const mode = process.argv[2];

/** The console build's location on the device, and the fixtures every page needs. */
function browserScript(checks) {
  return `const fs = require('fs');
const path = require('path');
const ROOT = 'C:\\\\ProgramData\\\\Vale\\\\pwout\\\\console';
const PROBE = ${JSON.stringify(PROBE_SOURCE)};
/* CHECKS */
const now = Date.now();
// The console's own render-smoke fixtures (gateway/ui/*-render-smoke.mjs), plus the two admin lists
// the keys/routes/users pages read. A missing stub renders that page's empty state, which is itself
// a surface worth grading — the judge does not treat "few nodes" as a pass.
const API = {
  '/api/me': { username: 'admin', role: 'admin', token: 'tok-abc', keys: { DEEPSEEK_API_KEY: { configured: true, masked: 'sk-\u20261' } } },
  '/api/me/keys': { keys: [
    { name: 'DEEPSEEK_API_KEY', configured: true, masked: 'sk-\u20261', usage: 12 },
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
  '/api/health': { channels: [{ prefix: 'og/', ok: true }] },
  '/api/admin/providers': { providers: [{ prefix: 'my/', label: 'My Provider', baseURL: 'https://api.example.com', api: 'openai-completions', models: [{ id: 'llama-3' }], advertised: ['my/llama-3'], keyEnv: '', keyMasked: 'sk-\u20269876', keyReady: true }], apis: [], filePrefixes: [] },
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

(async () => {
  const { acquireBrowser } = require(process.env.VALE_BROWSER_HELPER);
  const { page, close } = await acquireBrowser();
  await page.route('https://ai.saisi.online/**', (route) => {
    const p = new URL(route.request().url()).pathname;
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
  await page.setViewportSize({ width: 1440, height: 900 });
  const report = { rows: [], surfaces: [], names: [] };
  for (const [label, hash] of PAGES) {
    await page.goto('https://ai.saisi.online/?cb=' + Date.now(), { waitUntil: 'load' });
    await page.evaluate((h) => { location.hash = h; }, hash);
    await page.waitForTimeout(2000);
    const rows = await page.evaluate(PROBE);
    for (const r of rows) report.rows.push({ ...r, page: label, density: 'console', theme: 'light' });
    report.surfaces.push({ page: label, ...(await page.evaluate(SURFACE)) });
    report.names.push({ page: label, ...(await page.evaluate(NAMES)) });
  }
  fs.writeFileSync('C:\\\\ProgramData\\\\Vale\\\\pwout\\\\console-sweep.json', JSON.stringify(report));
  console.log(JSON.stringify({ rows: report.rows.length, surfaces: report.surfaces.length }));
  await close();
})().catch((e) => { console.error('FATAL', e.message); process.exit(1); });`.replace("/* CHECKS */", checks);
}

/** The in-page surface checks — the same shape the panel sweep uses, minus the session machinery. */
const PAGE_CHECKS = `
const SURFACE = \`(() => {
  const desc = (el) => el.tagName.toLowerCase() + (typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\\\\s+/).slice(0,2).join('.') : '');
  const heads = [...document.querySelectorAll('h1,h2,h3,h4')];
  const lv = heads.map((e) => Number(e.tagName.slice(1)));
  let skipped = 0;
  for (let i = 1; i < lv.length; i++) if (lv[i] - lv[i - 1] > 1) skipped++;
  const over = [], clipped = [], slivers = [];
  for (const el of document.querySelectorAll('#root *')) {
    const st = getComputedStyle(el);
    if (st.display === 'none' || st.visibility === 'hidden') continue;
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) continue;
    const own = [...el.childNodes].some((n) => n.nodeType === 3 && (n.textContent || '').trim().length > 0);
    const scrolls = st.overflowX === 'auto' || st.overflowX === 'scroll';
    const ellipsises = st.textOverflow === 'ellipsis';
    if (el.scrollWidth > el.clientWidth + 1 && !scrolls && !ellipsises) over.push(desc(el) + ' ' + el.clientWidth + '<' + el.scrollWidth);
    if (own && el.scrollWidth > el.clientWidth + 1 && !ellipsises) clipped.push(desc(el));
    const text = (el.textContent || '').trim();
    if (own && text.length > 24 && r.width < 60) slivers.push(desc(el) + ' w=' + Math.round(r.width));
  }
  return {
    h1Count: heads.filter((e) => e.tagName === 'H1').length,
    firstIsH1: heads.length > 0 && heads[0].tagName === 'H1',
    skipped, mains: document.querySelectorAll('main').length, navs: document.querySelectorAll('nav').length,
    over: [...new Set(over)].slice(0, 8), clipped: [...new Set(clipped)].slice(0, 8), slivers: [...new Set(slivers)].slice(0, 8),
  };
})()\`;

const NAMES = \`(() => {
  const SEL = 'button, a[href], input, select, textarea, [role="button"], [role="tab"], [role="switch"]';
  const name = (el) => {
    const by = el.getAttribute('aria-labelledby');
    if (by) { const t = by.split(/\\\\s+/).map((id) => (document.getElementById(id) || {}).textContent || '').join(' ').trim(); if (t) return t; }
    const label = el.getAttribute('aria-label'); if (label && label.trim()) return label.trim();
    if (el.tagName === 'INPUT' || el.tagName === 'SELECT' || el.tagName === 'TEXTAREA') {
      if (el.id) { const l = document.querySelector('label[for="' + CSS.escape(el.id) + '"]'); if (l && l.textContent.trim()) return l.textContent.trim(); }
      const wrap = el.closest('label'); if (wrap && wrap.textContent.trim()) return wrap.textContent.trim();
    }
    const text = (el.textContent || '').trim(); if (text) return text;
    // AN IMAGE WITH ALT TEXT NAMES ITS LINK. Without this branch the rail's brand (a link wrapping
    // <img alt="Vale">) read as "title-only" on every page — a false positive, and a detector that
    // cries wolf stops being read (round 47's lesson, applied to this sweep's own copy of the rule).
    const img = el.querySelector('img[alt]'); if (img && img.alt.trim()) return img.alt.trim();
    const title = el.getAttribute('title'); if (title && title.trim()) return 'title-only: ' + title.trim();
    return '';
  };
  const unnamed = [], titleOnly = [];
  let checked = 0;
  for (const el of document.querySelectorAll(SEL)) {
    const st = getComputedStyle(el);
    if (st.display === 'none' || st.visibility === 'hidden') continue;
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) continue;
    if (el.getAttribute('aria-hidden') === 'true') continue;
    checked++;
    const n = name(el);
    const d = el.tagName.toLowerCase() + (typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\\\\s+/)[0] : '');
    if (!n) unnamed.push(d);
    else if (n.startsWith('title-only:')) titleOnly.push(d + ' -> ' + n.slice(11));
  }
  return { checked, unnamed: [...new Set(unnamed)], titleOnly: [...new Set(titleOnly)] };
})()\`;
`;

function judge(file) {
  const report = JSON.parse(readFileSync(file, "utf8"));
  const findings = [];
  const under = failures(report.rows);
  if (under.length) {
    findings.push(
      `${under.length} text node(s) under AA:\n  ` +
        under.slice(0, 10).map((r) => `${r.cr} ${r.page} ${r.sel} "${String(r.text).slice(0, 24)}"`).join("\n  "),
    );
  }
  for (const s of report.surfaces) {
    if (s.h1Count !== 1 || !s.firstIsH1) findings.push(`${s.page}: h1 count ${s.h1Count}, first-is-h1 ${s.firstIsH1}`);
    if (s.skipped) findings.push(`${s.page}: ${s.skipped} skipped heading level(s)`);
    if (s.mains !== 1 || s.navs !== 1) findings.push(`${s.page}: ${s.mains} main, ${s.navs} nav`);
    for (const [kind, list] of [["overflow", s.over], ["clipping", s.clipped], ["sliver", s.slivers]]) {
      if (list && list.length) findings.push(`${s.page}: ${kind} — ${list.join("; ")}`);
    }
  }
  for (const n of report.names) {
    if (n.unnamed.length) findings.push(`${n.page}: ${n.unnamed.length} control(s) with NO accessible name — ${n.unnamed.join(", ")}`);
    if (n.titleOnly.length) findings.push(`${n.page}: ${n.titleOnly.length} control(s) named only by title — ${n.titleOnly.join(", ")}`);
  }
  const blind = unmeasurable(report.rows);
  console.log(
    `console: ${report.rows.length} text nodes · ${report.surfaces.length} pages · ${report.names.length} name checks` +
      (blind.length ? ` · ${blind.length} unmeasurable` : ""),
  );
  if (!findings.length) {
    console.log("console design sweep OK: nothing above found a defect");
    return 0;
  }
  console.error(`\n${findings.length} finding(s):\n  ` + findings.join("\n  "));
  return 1;
}

if (mode === "--emit") {
  process.stdout.write(browserScript(PAGE_CHECKS));
} else if (mode === "--judge") {
  const file = process.argv[3];
  if (!file) {
    console.error("usage: console-design-sweep.mjs --judge <report.json>");
    process.exit(2);
  }
  process.exit(judge(file));
} else {
  console.error(readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n").slice(1, 22).join("\n"));
  console.error("\nusage: console-design-sweep.mjs --emit | --judge <report.json>");
  process.exit(2);
}
