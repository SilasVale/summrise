#!/usr/bin/env node
// Render the REAL panel bundle with a stubbed device API, and audit it.
//
// WHY THIS EXISTS. Every earlier visual check in this repo was a hand-built HTML
// gallery: I wrote the markup, linked the stylesheet, and measured the elements I
// had just created. That verifies the CSS I was thinking about and nothing else —
// and it is how five chrome contrast defects lived through several rounds of
// "auditing": `#session-count`, `.side-time`, `.side-count`, `.tab.active` and
// `.view-switch-btn.active` were wrong the whole time and no feature-by-feature
// gallery could see them, because I only ever measured what I was working on.
//
// This harness instead loads `resources/panel/panel.js` and `panel.css` — the
// exact bytes the agent embeds via include_str! — into a page that serves at a
// real `/panel/` origin, with `window.fetch` stubbed to return a fixed device
// state. The app boots through its own production path (including computeBoot's
// same-origin branch) and renders its own component tree. Then it:
//
//   1. measures EVERY visible text node, alpha-compositing both background alpha
//      and the ancestor `opacity` chain (element opacity is in NEITHER
//      getComputedStyle(color) NOR backgroundColor — a probe that ignores it
//      reports a dimmed element at its full colour);
//   2. asserts each governance element is PRESENT, so a clean contrast sweep over
//      a page that failed to render cannot pass;
//   3. checks nothing in the top bar overflows.
//
// Pages are addressed over http://vale.test/panel/ and satisfied by Playwright
// route interception, so NO listener is opened anywhere — the run is safe on a box
// where binding a port is not allowed.
//
// Usage (needs the Playwright runtime; see VALE_BROWSER_HELPER):
//   node scripts/panel-render-audit.mjs --out /tmp/panel-audit
//
// Regenerate panel.js/panel.css first: (cd agent/resources/panel-react && npm run build)


import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const PANEL = join(ROOT, "agent", "resources", "panel");
const OUT = (() => {
  const i = process.argv.indexOf("--out");
  return i >= 0 ? process.argv[i + 1] : "/tmp/panel-render-audit";
})();

const SID = "term-audit-0";

// A device state that exercises every governance surface at once: a stated goal,
// the gate armed, a command waiting for a decision, a grant already in force, and
// a trail carrying intent + the branches not taken.
const EVENTS = [
  { seq: 1, ts: 1789000000, kind: "status", status: "opened" },
  { seq: 2, ts: 1789000001, kind: "goal", text: "provision the ONU at 0/1 on VLAN 100, then save the config" },
  { seq: 3, ts: 1789000002, kind: "approval", status: "armed" },
  {
    seq: 4, ts: 1789000010, kind: "command/start", command: "display ont info 0 1",
    intent: "check whether the ONU is actually online before changing its config",
    considered: ["reset the ONU", "check the OLT uplink first"],
  },
  { seq: 5, ts: 1789000011, kind: "output", text: "ONT 0/1 online, VLAN 1" },
  { seq: 6, ts: 1789000012, kind: "command/end", exit_code: 0, duration_ms: 900 },
  { seq: 7, ts: 1789000020, kind: "approval", status: "approved", text: "vlan 100" },
  { seq: 8, ts: 1789000021, kind: "approval", status: "granted", text: "vlan" },
  { seq: 9, ts: 1789000030, kind: "command/start", command: "vlan 100", intent: "apply the change" },
  { seq: 10, ts: 1789000031, kind: "command/end", exit_code: 1, duration_ms: 200 },
];

const SESSION = {
  id: SID, kind: "pty", label: "d1", status: "live", bytes: 512,
  held_by_human: false, approval_required: true, approval_grants: ["display"],
  goal: "provision the ONU at 0/1 on VLAN 100, then save the config",
  pending_approval: {
    id: "ap-1",
    command: "vlan 100 / port vlan 100 0/1 1",
    expires_in_ms: 47000,
  },
};

/** Elements the governance surface must render. A contrast sweep that measured a
 *  blank page would otherwise be a clean, meaningless PASS. */
const REQUIRED = [
  ["goal bar", "#goal-bar"],
  ["goal text", ".goal-text"],
  ["approval prompt", ".approval-prompt"],
  ["pending command", ".approval-cmd"],
  ["countdown", ".approval-left"],
  ["approve button", ".approval-approve"],
  ["refuse button", ".approval-refuse"],
  ["remember button", ".approval-remember"],
  ["session tab", ".tab-name"],
  ["view switch", ".view-switch-btn.active"],
  ["status session count", "#session-count"],
  ["side time", ".side-time"],
  ["side count", ".side-count"],
];

function buildHarness() {
  const css = readFileSync(join(PANEL, "panel.css"), "utf8");
  // WHAT THIS HARNESS CANNOT REACH, measured rather than assumed (round 45): the evidence drawer —
  // and with it the browser-action badges and the screenshot timestamp — renders ONLY in the
  // Electron shell, because BrowserPage mounts its pane behind `window.valeEmbedded` and a plain
  // browser gets an explanation page instead. A fixture for it was tried and removed: it could not
  // render, and a dead fixture is a lie about coverage. Those three badge inks were therefore fixed
  // on the STATIC pair sweep's measurement (1.99 -> 5.73+ on the light chrome surface) and are
  // confirmed there and in the token contract; the rendered confirmation needs the desktop app.
  const js = readFileSync(join(PANEL, "panel.js"), "utf8");
  // No closing script tag may appear in ANY inline script, and there are two of them: the bundle
  // here and the stub below. Checking only the bundle is how a comment in the stub silently cut the
  // whole fixture in half — the page still loaded, still rendered a panel, and simply had no
  // sessions, which reads like a product bug rather than a broken harness.
  if (/<\/script/i.test(js)) throw new Error("panel.js contains a closing script tag — inline embedding is unsafe");

  const stub = `
// NO BACKTICKS IN THIS TEMPLATE, AND NO CLOSING SCRIPT TAG EITHER — not even inside a comment.
// A backtick ends the literal and the file stops parsing; a closing script tag ends the HTML tag
// and the rest of this stub is silently dropped by the parser (measured: round 41, three times,
// twice in comments that were explaining something else). Quote identifiers with 'single quotes'.
(function(){
  var P = new URLSearchParams(location.search);
  var THEME = P.get('theme') || 'light', MODE = P.get('mode') || 'pending';
  try { localStorage.setItem('vale-theme', THEME); } catch(e){}
  window.__PANEL_TOKEN__ = 'audit-token';
  var SID = ${JSON.stringify(SID)}, SESSION = ${JSON.stringify(SESSION)}, EVENTS = ${JSON.stringify(EVENTS)};
  if (MODE === 'idle') SESSION = Object.assign({}, SESSION, {pending_approval: null});
  // THE APPROVAL CLOCK, three states a person actually sees (round 46). The default fixture carries
  // expires_in_ms: 47000, which the gate reads as URGENT (under its one-minute threshold) — so the
  // alarmed state was the only one any sweep had ever rendered. 'relaxed' is a question with time
  // left, 'expired' is the zero-second rule: the device has retired it and nothing can be answered.
  // ?fail=1 — the device is DOWN: every API call rejects, which is what a page shows an operator
  // when the agent is not running. No sweep had ever produced this state.
  var FAIL = P.get('fail') === '1';
  // ?held=1 — A PERSON HOLDS THE KEYBOARD. The AI is refused on this session while it is set, so the
  // panel's job is to make that state unmistakable; no sweep had ever rendered it (the fixture
  // always said false).
  if (P.get('held') === '1') SESSION = Object.assign({}, SESSION, {held_by_human: true});
  if (MODE === 'relaxed') SESSION = Object.assign({}, SESSION, { pending_approval: Object.assign({}, SESSION.pending_approval, { expires_in_ms: 600000 }) });
  if (MODE === 'expired') SESSION = Object.assign({}, SESSION, { pending_approval: Object.assign({}, SESSION.pending_approval, { expires_in_ms: 0 }) });
  // ?sessions=N — MEASURE THE TAB STRIP AT A REALISTIC WIDTH. The operator's own panel carried
  // ELEVEN tabs, and a strip with one tab says nothing about overflow, truncation or whether the
  // close affordance survives a crowd. Default 1 so every existing check keeps its baseline.
  var WANT = parseInt(P.get('sessions') || '1', 10);
  var SESSIONS = [SESSION];
  for (var i = 1; i < WANT; i++) {
    SESSIONS.push(Object.assign({}, SESSION, {
      // IDLE TIME, as the device reports it since round 37 (terminal_list.idle_ms). No backticks
      // here: this is INSIDE a template literal, and one would end the stub — the same trap the
      // contrast probe's own header documents.
      // The first session stays active; the rest are silent for hours — the state the
      // panel's "nobody is using these" offer exists for. A fixture with no idle data
      // cannot show that offer at all.
      idle_ms: i * 20 * 60 * 1000,
      id: 'term-audit-' + i,
      label: i % 3 === 0 ? 'stc@192.168.1.1' : (i % 3 === 1 ? 'serial:COM4' : 'pwsh'),
      kind: i % 3 === 0 ? 'ssh' : (i % 3 === 1 ? 'serial' : 'pty'),
      pending_approval: null,
      goal: '',
    }));
  }
  var J = function(o){ return new Response(JSON.stringify(o), {status:200, headers:{'content-type':'application/json'}}); };
  // WHAT THE PANEL ACTUALLY CALLED. A reader watching Playwright's network events sees NOTHING —
  // this stub answers in-page, so the only witness to "did that button really close seven sessions"
  // is the stub itself. (Round 41: the first version of this verification reported closeCalls: 0
  // and the action HAD run; the counter was looking at the wrong layer. No backticks in this
  // file's stub comments — see the warning at the top of the template.)
  window.__calls = [];
  // Recorded entries are 'tools/terminal_close {json}'; match the NAME ANYWHERE, not by equality —
  // the first version compared the whole string to the bare name, so a run that closed seven
  // sessions reported zero and the product looked broken while the counter was.
  window.__callCount = function(name){ return window.__calls.filter(function(c){ return c.indexOf(name) >= 0; }).length; };
  window.__callBody = function(name){ return window.__calls.filter(function(c){ return c.indexOf(name) >= 0; }); };
  var realFetch = window.fetch.bind(window);
  window.fetch = function(url, init){
    var u = String(url);
    if (FAIL && u.indexOf('/api/') >= 0) return Promise.reject(new TypeError('Failed to fetch'));
  // THE PLUGINS PAGE, POPULATED. /api/spec and /api/plugins/status were never stubbed, so every
  // sweep before round 78 rendered that page in its "inventory could not be read" state and the
  // populated cards — names, descriptions, tool counts, the playwright block — had never been
  // measured at all. Round 69's lesson, applied to the pages it did not cover.
  if (u.indexOf('/api/spec') >= 0) {
    return Promise.resolve(J({ plugins: [
      { name: 'terminal', displayName: 'Terminal', description: 'PTY, SSH and serial sessions with a durable audit trail.', tools: [{ name: 'terminal_execute' }, { name: 'terminal_list' }, { name: 'terminal_read' }, { name: 'terminal_write' }, { name: 'terminal_open' }, { name: 'terminal_close' }] },
      { name: 'system', displayName: 'System', description: 'Files, processes, services and the registry.', tools: [{ name: 'system_file_read' }, { name: 'system_file_write' }, { name: 'system_process_list' }] },
      { name: 'memory', displayName: 'Memory', description: 'What the AI has been asked to remember on this device.', tools: [{ name: 'memory_set' }, { name: 'memory_list' }] },
      { name: 'runs', displayName: 'Runs', description: 'Long-running commands and their progress.', tools: [] },
    ] }));
  }
  if (u.indexOf('/api/plugins/status') >= 0) {
    return Promise.resolve(J({ ok: true, playwright: {
      version: '1.56.0', core: '1.56.0', installed: true, browser: 'chromium-1187',
      ready: true, downloads: [{ name: 'chromium', state: 'ready' }],
    } }));
  }
  // THE MEMORY PAGE, POPULATED. It reads through the TOOL route (POST /api/tools/memory_list,
  // answered by callTool), which the stub also never served — so this page, like the plugins one,
  // had only ever been rendered empty. Same lesson, same round. (No backticks in here: this text
  // lives INSIDE the emitted template literal, and a stray one ends it — the sixth time.)
  if (u.indexOf('/api/tools/memory_list') >= 0) {
    return Promise.resolve(J({ ok: true, result: { results: [
      { id: 'mem-1', title: 'Router admin host', content: 'The NP3081G router answers on 192.168.1.1 with SSH user stc.', tags: ['network', 'router'], namespace: 'default', source: 'chat', created_at: 1789000000, updated_at: 1789000000 },
      { id: 'mem-2', title: 'Build box toolchain', content: 'The Windows exe is cross-compiled with cargo xwin; the toolchain is pinned in rust-toolchain.toml.', tags: ['build'], namespace: 'default', source: 'chat', created_at: 1788990000, updated_at: 1788990000 },
      { id: 'mem-3', title: 'Serial console framing', content: 'COM4 runs 115200 8N1 for the ONT console.', tags: ['serial', 'ont'], namespace: 'default', source: 'auto', created_at: 1788980000, updated_at: 1788980000 },
    ] } }));
  }
  // THE MONITORS SURFACE, WHICH HAD NEVER BEEN RENDERED. /api/monitors was served by nothing, so the
  // alert strip and the monitor card had only ever been measured in their empty state — the same gap
  // rounds 69 and 78 found on two other surfaces, and both times it was real. Round 100.
  if (u.indexOf('/api/monitors') >= 0 && u.indexOf('/api/monitors/') < 0) {
    var probe = function (i, ok, ms) { return { ts_ms: 1789000000000 + i * 15000, ok: ok, ms: ms }; };
    return Promise.resolve(J({ targets: [
      {
        id: 'mon-router', host: '192.168.1.1', port: 22, path: null, expect: null,
        summary: { probes: 240, up: 239, down: 1, up_pct: 99.6, up_now: true, since_ms: 1789000000000, drops: 1,
                   latency: { min: 3, avg: 11, max: 88 }, last_status: null, body_ok: null },
        transitions: [{ at_ms: 1788990000000, up: false, lasted_ms: 300000 }],
        series: [probe(0, true, 9), probe(1, true, 12), probe(2, true, 8)],
      },
      {
        id: 'mon-ont', host: '192.168.1.1', port: 8000, path: '/status', expect: 'ONT', path_label: 'http',
        summary: { probes: 240, up: 180, down: 60, up_pct: 75.0, up_now: false, since_ms: 1789002000000, drops: 3,
                   latency: { min: 40, avg: 120, max: 900 }, last_status: 502, body_ok: false },
        transitions: [
          { at_ms: 1789002000000, up: false, lasted_ms: 900000 },
          { at_ms: 1788995000000, up: true, lasted_ms: 600000 },
        ],
        series: [probe(0, true, 120), probe(1, false, null), probe(2, false, null)],
      },
    ] }));
  }
  // ?rows=N — the ARCHIVE with content, at scale. Every sweep until round 69 answered /api/sessions
  // with a bare {} (the stub's generic branch), so the History page has only ever been measured
  // EMPTY: the page's cost with a device that has recorded hundreds of sessions was unknown.
  if (u.indexOf('/api/sessions') >= 0) {
    var want = parseInt(P.get('rows') || '0', 10);
    var rows = [];
    for (var r = 0; r < want; r++) {
      rows.push({
        id: 'term-arch-' + r,
        kind: (r % 3 === 0) ? 'ssh' : (r % 3 === 1 ? 'serial' : 'pty'),
        label: (r % 3 === 0) ? 'stc@192.168.1.1' : (r % 3 === 1 ? 'serial:COM4' : 'pwsh'),
        state: { ended_ms: 1789000000000 - r * 60000, exit_code: (r % 7 === 0) ? 1 : 0 },
      });
    }
    return Promise.resolve(J({ sessions: rows }));
  }
    var body = (init && init.body) ? String(init.body) : '';
    // Double backslash: this is inside a template literal, where a single \/ collapses to / and the
    // emitted regex becomes /^.*/api// — "Invalid regular expression flags", which killed the WHOLE
    // stub (no token, no sessions, no counter) and looked from the outside like a broken product.
    window.__calls.push(u.replace(/^.*\\/api\\//, '') + ' ' + body);
    if (u.indexOf('/api/tools/terminal_list') >= 0)    return Promise.resolve(J({ok:true, result:SESSIONS}));
    if (u.indexOf('/api/tools/terminal_history') >= 0) return Promise.resolve(J({ok:true, result:[]}));
    if (u.indexOf('/api/tools/terminal_read') >= 0)    return Promise.resolve(J({ok:true, result:{text:'ONT 0/1 online', start:0, end:14, evicted:false}}));
    if (u.indexOf('/api/tools/') >= 0)                 return Promise.resolve(J({ok:true, result:'OK'}));
    if (/\\/api\\/sessions\\/[^/]+$/.test(u))            return Promise.resolve(J({ok:true, id:SID, events:EVENTS}));
    if (u.indexOf('/api/plugins/status') >= 0)         return Promise.resolve(J({plugins:[{name:'terminal',ok:true}]}));
    if (u.indexOf('/api/spec') >= 0)                   return Promise.resolve(J({plugins:[]}));
    if (u.indexOf('/api/') >= 0)                       return Promise.resolve(J({ok:true}));
    return realFetch(url, init);
  };
  window.EventSource = function(){ this.addEventListener=function(){}; this.removeEventListener=function(){}; this.close=function(){}; };
  window.WebSocket = function(){ this.addEventListener=function(){}; this.send=function(){}; this.close=function(){}; };
})();`;

  if (/<\/script/i.test(stub)) throw new Error("the stub contains a closing script tag — it would cut the fixture short");
  return `<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Vale Agent</title>
<style>${css}</style></head><body><div id="root"></div>
<script>${stub}</script><script type="module">${js}</script></body></html>`;
}

const OVERFLOW = `(() => {
  const bad = [];
  for (const el of document.querySelectorAll('#root *')) {
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) continue;
    if (el.scrollWidth > el.clientWidth + 1) {
      const cls = typeof el.className === 'string' ? el.className.trim().split(/\\s+/)[0] : '';
      bad.push(el.tagName.toLowerCase() + (cls ? '.' + cls : '') + ' over=' + (el.scrollWidth - el.clientWidth));
    }
  }
  return [...new Set(bad)].slice(0, 12);
})()`;

async function main() {
  mkdirSync(OUT, { recursive: true });
  const html = buildHarness();
  const harnessPath = join(OUT, "panel-harness.html");
  writeFileSync(harnessPath, html);

  // WHERE THIS RUNS. The audit needs a Playwright runtime, and on the Linux dev
  // box there is none that can launch (the system chromium is missing
  // libatk-1.0.so.0 and there is no sudo). The runtime that DOES work is the
  // device's bundled one, reached through the agent's `browser_run_script` tool —
  // which is how every round of this work actually audited the panel. So:
  //
  //   * with VALE_BROWSER_HELPER set (on a device, or any box with the bundle)
  //     this script runs the whole audit itself;
  //   * without it, it still generates the harness and prints the probe, so the
  //     same measurement can be driven from wherever a runtime exists.
  //
  // Emitting rather than failing is the point: a check that can only run in one
  // environment is a check that quietly stops running.
  const helper = process.env.VALE_BROWSER_HELPER;
  if (!helper) {
    console.log("No VALE_BROWSER_HELPER — harness written, running in EMIT mode.");
    console.log("  harness: " + harnessPath);
    console.log("  drive it by loading that file in any Playwright page, routing");
    console.log("  http://vale.test/** to its body, then evaluating the PROBE from");
    console.log("  lib/contrast-probe.mjs and the OVERFLOW snippet in this file.");
    console.log("  (The PROBE moved out of this file so its math can be UNIT TESTED —");
    console.log("   scripts/test/contrast-probe-check.mjs exercises the exact text the");
    console.log("   browser evaluates.)");
    // A SKIP IS NOT A PASS, AND THE EXIT CODE SAYS WHICH.
    //
    // This used to exit 0, and the message above used to admit it: "a caller
    // watching only the exit code reads this skip as a pass". Writing the defect
    // down and leaving it is how it survived — the sentence read as a caveat
    // rather than as a bug.
    //
    // The convention, now explicit and pinned by scripts/test/panel-audit-skip-check.mjs:
    //   0  the audit RAN and found nothing
    //   1  the audit RAN and found failures
    //   2  the audit DID NOT RUN (no Playwright runtime in this environment)
    //
    // 2 is deliberately NOT 0. Emitting the harness instead of failing is still
    // right — a check that can only run in one environment quietly stops running —
    // but "I could not look" must be distinguishable from "I looked and it is
    // clean", and the exit code is the only channel a caller is guaranteed to read.
    console.log("EXIT 2: THE AUDIT DID NOT RUN — this is a SKIP, not a pass.");
    process.exit(2);
  }
  const { acquireBrowser } = await import(helper);
  const { page, close } = await acquireBrowser();

  // Served at a REAL origin and path, satisfied by interception — the panel sees
  // location.pathname === "/panel/" and boots through its own production branch.
  // No listener is opened anywhere.
  await page.route("http://vale.test/**", (route) =>
    route.fulfill({ status: 200, contentType: "text/html; charset=utf-8", body: html }),
  );

  const pageErrors = [];
  page.on("pageerror", (e) => pageErrors.push(String(e.message).slice(0, 200)));

  let failures = 0;
  let measured = 0;
  let low = 0;

  for (const theme of ["light", "dark"]) {
    for (const mode of ["pending", "idle"]) {
      await page.setViewportSize({ width: 1280, height: 720 });
      await page.goto(`http://vale.test/panel/?theme=${theme}&mode=${mode}`, { waitUntil: "load" });
      await page.waitForTimeout(1800);

      const rows = await page.evaluate(PROBE);
      const under = rows.filter((r) => r.cr < 4.5);
      measured += rows.length;
      low += under.length;
      console.log(`\n--- ${theme} / ${mode}: ${rows.length} text nodes, ${under.length} under AA ---`);
      for (const r of under) {
        failures++;
        console.log(`  LOW ${String(r.cr).padStart(5)}  ${String(r.size).padStart(4)}px  ${r.sel.slice(0, 46)}  "${r.text}"`);
      }

      // Presence: a clean sweep over a page that did not render proves nothing.
      if (mode === "pending") {
        const found = await page.evaluate(
          (sels) => Object.fromEntries(sels.map(([k, s]) => [k, !!document.querySelector(s)])),
          REQUIRED,
        );
        for (const [k] of REQUIRED) {
          if (!found[k]) {
            failures++;
            console.log(`  MISSING  ${k}`);
          }
        }
        const over = await page.evaluate(OVERFLOW);
        for (const o of over) {
          failures++;
          console.log(`  OVERFLOW ${o}`);
        }
        if (pageErrors.length) {
          failures++;
          console.log(`  PAGE ERRORS ${JSON.stringify(pageErrors.slice(0, 3))}`);
        }
        await page.screenshot({ path: join(OUT, `panel-${theme}.png`), fullPage: false });
      }
    }
  }

  console.log(`\n== ${measured} text nodes measured, ${low} under AA, ${failures} failure(s) ==`);
  console.log(`screenshots: ${OUT}/panel-light.png, ${OUT}/panel-dark.png`);
  await close();
  process.exit(failures ? 1 : 0);
}

main().catch((e) => {
  console.error("FATAL " + e.message);
  process.exit(1);
});
