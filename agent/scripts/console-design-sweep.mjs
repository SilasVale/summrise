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
//   clipping and slivers at 1440/900/720/640/320 · WCAG reflow at the width the criterion names · accessible names · keyboard focus rings.
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
import { readFileSync, readdirSync } from "node:fs";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { failures, unmeasurable, PROBE_SOURCE } from "./lib/contrast-probe.mjs";
import {markCoverageNotes, pageChecks, judgeReport, reportSummary, UNSTYLED_SOURCE, focusPass, ackPass, pressPass, idlePass, motionPass, TARGETS_SOURCE, THEME_SOURCE, DIAG_SOURCE, pressDelta, discoverPressTargets, assertEmbedded } from "./lib/design-sweep.mjs";

const mode = process.argv[2];
// WHICH AXES TO RUN. The panel sweep has had this since round 31 and the console had none: every run measured
// everything, which took ~95 s and did not fit in one tool call — so an axis could only be checked by paying for all
// of them. `--passes=press` is what makes a single axis measurable; the default is every axis, unchanged.
// BAKED INTO THE EMITTED SCRIPT below, because that is where the gates are: a helper defined out here does not
// exist on the device (the first run of this failed with "wants is not defined").
const PASSES = (process.argv.find((a) => a.startsWith('--passes=')) || '').slice('--passes='.length).split(',').filter(Boolean);

// HOW TO RENDER THIS CONSOLE FROM HERE WITHOUT DELIVERING A DIRECTORY (round 17 of the standing goal). The sweep
// below measures a DELIVERED build — `C:\ProgramData\Vale\pwout\console`, several files, one transfer each. For a
// one-off look at a page there is a cheaper path, and it was used to verify the `.sig-dot` silhouettes on the real
// console (ok = circle, err = a ROTATED diamond at 2px radius; inks 6.10 and 4.65 against their own cards):
//
//   1. `gateway/public/` holds the whole build. Emit ONE file: a JSON map of path -> contents for `index.html`,
//      `style.css`, `favicon.svg`, `icons.svg` and everything in `assets/` (about 390 KB, one transfer).
//   2. On the device, route `http://vale.test/**` by looking the pathname up in that map, with `/` and any
//      extension-less path falling back to `/index.html` (it is an SPA) and a 404 for a missing FILE.
//   3. Route `/api/**` ON THE SAME ORIGIN. The built console calls `/api/me`, not the deployed host — the sweep's
//      own handler keys on `https://ai.saisi.online`, which is right for ITS delivery and wrong for this one.
//
//   TWO TRAPS, both of which cost an attempt here:
//     * INLINING THE BUNDLE INTO THE HTML DOES NOT WORK. The bundle contains `</script>` and `<!--` sequences that
//       end the script element early however carefully the closing tags are escaped — the page renders the source
//       as text. Serve it as a FILE and let the browser fetch it as a module.
//     * KEEP `type="module"`. The entry tag is `<script type="module" crossorigin src=...>`; dropping the attribute
//       makes the ESM bundle a classic script and nothing mounts (`#root` stays empty with no error).
//
// COMPUTED AT EMIT TIME, NOT BAKED (round 223). Round 191 wrote the entry digest as a LITERAL, so
// rebuilding the UI changed the file and made the check unsatisfiable — a fix for exactly this was made
// for the panel harness in round 210 and NOT applied here, which is why the console sweep failed on the
// first rebuild after that. The value is computed in the module (where the repository is) and only its
// result is inlined into the emitted script.
// ONE ROOT, BOTH ENDS (round 77). The stamp is baked from the entry the RUN will serve — the same
// `VALE_SWEEP_ROOT` the sweep reads at run time — and until now it was read from a hard-coded
// `gateway/public`, the copy the RELEASE flow writes. So a CI run that built the console from its own
// checkout and served that while the stamp came from the last published build reported every run as
// "stale" — a true statement about two artefacts and a useless one about a commit. The default is
// unchanged, so a device run compares the delivered copy against the delivered copy it was emitted for.
const ENTRY_STAMP = (() => {
  const root = process.env.VALE_SWEEP_ROOT || new URL("../../gateway/public", import.meta.url).pathname;
  try {
    const b = readFileSync(new URL("index.html", new URL(root.endsWith("/") ? root : root + "/", "file://")));
    return { bytes: b.length, sha: createHash("sha256").update(b).digest("hex").slice(0, 12) };
  } catch (e) { return { bytes: -1, sha: "(unreadable)" }; }
})();
/** The console's state sheets, concatenated: its `dist` is a pruned build artifact, so the SOURCE sheets are what the
 *  mark-coverage note reads (the same choice `console-marks-check.mjs` makes, for the same reason). */
function consoleSheets() {
  const dir = new URL("../../gateway/ui/src/styles/", import.meta.url).pathname;
  return readdirSync(dir)
    .filter((f) => f.endsWith(".css"))
    .map((f) => readFileSync(dir + f, "utf8"))
    .join("\n");
}

function browserScript() {
  const script = `const fs = require('fs');
const path = require('path');
// WHERE IT READS THE BUILT UI IS OVERRIDABLE, so the same sweep runs on the device (the default, exactly
// as before) or in CI against the repository's own build with nothing delivered in between — which is
// what makes the design checks continuous rather than remembered (rounds 204, 219). The panel's sweep
// has taken its paths from the environment since round 204; these two follow it.
const ROOT = process.env.VALE_SWEEP_ROOT || 'C:\\\\ProgramData\\\\Vale\\\\pwout\\\\console';
const REPORT_PATH = process.env.VALE_SWEEP_REPORT || 'C:\\\\ProgramData\\\\Vale\\\\pwout\\\\console-sweep.json';

// THE ENTRY THIS SWEEP WAS EMITTED AGAINST. Both UIs are measured from a DELIVERED copy of their
// build, and nothing said which generation it was: round 184 found the console's directory holding eight
// files from four generations, and round 189 lost an afternoon to a stale PANEL harness whose collapsed
// tab strip read as a live regression. The panel's harness now stamps itself; these two carry the entry's
// digest instead, because a stale delivery always shows up in the file that names everything else.
const EXPECTED_ENTRY = ${JSON.stringify(ENTRY_STAMP)};
// DERIVED FROM ROOT, NOT BAKED. Round 191 wrote this as the device path, so when round 219 made ROOT
// overridable the check kept looking at C:\ProgramData\Vale while the sweep served the repository — and
// CI reported ENOENT for a file that was right there. A check that names a location must follow the same
// override the thing it checks does.
const EXPECTED_ENTRY_PATH = require("path").join(ROOT, "index.html");
const PROBE = ${JSON.stringify(PROBE_SOURCE)};
const UNSTYLED = ${JSON.stringify(UNSTYLED_SOURCE)};
const focusPass = ${focusPass.toString()};
const pressDelta = ${pressDelta.toString()};
const pressPass = ${pressPass.toString()};
const ACK_BUDGET_MS = 100;
// AND THE ACK PASS (round 190). Same helper the panel uses, embedded the same way: it times the gap between a
// press and the first visible acknowledgement against the stated budget, and with discover it asks the DOM for every
// visible control instead of a list somebody thought of.
const ackPass = ${ackPass.toString()};
// AND THE HELPER pressPass CALLS: it asks the DOM for the page's controls. A borrowed helper that calls another
// one needs that one embedded too, or the run dies on the device with "is not defined" — the failure the emitted
// check below exists for.
const discoverPressTargets = ${discoverPressTargets.toString()};
const idlePass = ${idlePass.toString()};
const TARGETS = ${JSON.stringify(TARGETS_SOURCE)};
const THEME = ${JSON.stringify(THEME_SOURCE)};
// THE SWEEP REPORTS ITSELF TO THE AGENT'S DIAGNOSTIC RING, so a caller whose tool call timed out can tell
// a run that is still working from one that was killed (round 181 lost half an hour to exactly that).
${DIAG_SOURCE}
const motionPass = ${motionPass.toString()};
${pageChecks("#root")}
const PASSES = ${JSON.stringify(PASSES)};
const wants = (name) => !PASSES.length || PASSES.includes('all') || PASSES.includes(name);
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
  // THE FIELD THE PAGE ACTUALLY READS (round 53 of the standing goal). This said verdict: 'crashed', and
  // DevicesPanel reads st?.last_boot_kind === "crashed" — a field name the page does not read is a fixture saying
  // something nothing hears, so the crash row never rendered and sig-dot.err (ONE OF THREE STATES in that family) had
  // no surface in 136 sweeps. The gateway sends the real pair (plugins/mcp.ts forwards the probe's
  // lastBootKind/lastBoot), which is why this is a FIXTURE fix and not a product one: measured before changing it.
  // THE PUBLIC ROUTES, WHICH THE MODELS PAGE CANNOT RENDER WITHOUT (round 57 of the standing goal). Its provider rows
  // come from api.getPublicRoutes() (/api/admin/public), and Models.tsx sets failed when info.models is empty
  // — so with no fixture for this endpoint the page rendered its failure banner and NO ROWS, which is why prov-dot
  // (two declared states) had no surface in 136 sweeps while every other check stayed green. The body is the one the
  // console's own models-render-smoke.mjs uses, which is what this fixture table claims to be: the same /api bodies
  // the render smokes assert against in jsdom. my/ pairs with the provider below it (keyReady) so prov-dot.ok
  // renders, and the og/ + none rows give prov-dot.missing.
  // ONE ENTRY, NOT TWO (round 59). This key was in the table TWICE: the body below, and a later { enabled: false }
  // stub — and in a JS object literal the LAST key wins, so the real body never reached the page. Models.tsx needs
  // models/routes (it sets failed without them) while something else read enabled, so both fields live here.
  // A duplicate key in a fixture table is the two-copies-of-one-fact defect this objective exists to remove, and this
  // one was MINE: round 57 added the body without checking whether the key already existed.
  '/api/admin/public': {
    enabled: false,
    models: ['og/deepseek/deepseek-v4.1-flash', 'my/llama-3', 'deepseek/deepseek-v4.1-flash'],
    // or/ IS THE ROW THE OTHER DOT STATE NEEDS (round 62): /api/health already reports that channel as
    // ok: false, and ready = h?.ok !== false is what turns that into prov-dot.missing. Without a route for it the
    // failing channel had nothing to mark, which is why the family rendered only ok — a fixture that exists and no
    // row to apply it to, one step past the duplicate-key bug that hid the whole page.
    routes: [
      { prefix: 'og/', backend: 'og', models: ['deepseek/deepseek-v4.1-flash'] },
      { prefix: 'my/', backend: 'my', models: ['llama-3'] },
      { prefix: 'or/', backend: 'or', models: ['deepseek/deepseek-v4.1-flash'] },
      { prefix: 'none', backend: '', models: ['deepseek/deepseek-v4.1-flash'] },
    ],
  },
  '/api/plugins/status': { devices: { d1: { online: false, agent_up: true, tunnel_up: true, version: '1.0.106', checked_at: now, last_boot_kind: 'crashed', last_boot: 'run journal: previous run DID NOT EXIT CLEANLY — CRASHED or was killed; survived 61s' } } },
  // ALL FOUR CHANNELS, because the lane rules are per-channel: with only 'og' in the fixture the
  // three fills that do NOT flip with the theme never render, and a contrast fix for them could not
  // be seen. Measured round 79 — the ink/fill pairing differs per lane on purpose.
  // THE CHANNELS ARE KEYED BY id, WHICH IS WHAT THE PAGE READS (round 64): Models.tsx looks a row's health up with
  // health.find((h) => h.id === prefix || h.id === prefix.replace(/\/$/, "")), and this fixture wrote prefix:
  // instead — so every lookup missed, h was undefined, h?.ok !== false was TRUE, and prov-dot.missing could not
  // render however many failing channels the fixture carried. (or/ is the failing one; the others are healthy.)
  '/api/health': { channels: [
    { id: 'og/', ok: true },
    { id: 'ds/', ok: true },
    { id: 'or/', ok: false },
    { id: 'qw/', ok: true },
  ] },
  '/api/admin/providers': { providers: [{ prefix: 'my/', label: 'My Provider', baseURL: 'https://api.example.com', api: 'openai-completions', models: [{ id: 'llama-3' }], advertised: ['my/llama-3'], keyEnv: '', keyMasked: 'sk-9876', keyReady: true }], apis: [], filePrefixes: [] },
  '/api/admin/models': { models: [{ id: 'my/llama-3', label: 'llama-3' }] },
  '/api/admin/catalogue': { models: [{ id: 'my/llama-3', label: 'llama-3' }] },
  '/api/admin/users': { users: [{ username: 'operator', role: 'admin', createdAt: now - 86400000 }, { username: 'guest', role: 'user', createdAt: now - 3600000 }] },
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
// EVERY API CALL FAILS, for the pass that renders the console's error surfaces. Same shape as the empty and
// auth flags: a flag the route handler reads, not a fixture. The panel has had this since round 160 (?fail=1) and
// the console never did — so nothing had rendered what an operator sees when the worker cannot reach a device.
const fail = { api: false };
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
    // /api/me IS EXEMPT, AND THAT IS THE WHOLE DIFFERENCE BETWEEN AN ERROR STATE AND A LOGIN PAGE. Failing every
    // call made the app believe nobody was signed in, so all six pages rendered the login screen — which has no
    // nav by design, and the first run of this pass reported exactly that six times. The panel's ?fail=1 fails
    // DEVICE calls, never auth; this does the same. The login page has its own pass.
    if (fail.api && p.startsWith('/api/') && p !== '/api/me') {
      return route.fulfill({ status: 500, contentType: 'application/json', headers: { 'cache-control': 'no-store' }, body: JSON.stringify({ type: 'error', error: { message: 'the device is unreachable' } }) });
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
  await diag("start console pid=" + process.pid);
  const report = { rows: [], surfaces: [], names: [], focus: [], press: [], idle: [], reflow: [], hover: [], unstyled: [], motion: [], targets: [], themeChecks: [] , entryCheck: (() => { try { const b = fs.readFileSync(EXPECTED_ENTRY_PATH); const c = require("crypto").createHash("sha256").update(b).digest("hex").slice(0, 12); return { bytes: b.length, sha: c, expected: EXPECTED_ENTRY, stale: b.length !== EXPECTED_ENTRY.bytes || c !== EXPECTED_ENTRY.sha }; } catch (e) { return { error: String(e.message).slice(0, 60), expected: EXPECTED_ENTRY, stale: true }; } })() };
  // THE CONSOLE IN DARK. It has a dark theme — body[data-theme=dark], applied before the first paint and
  // persisted in localStorage — and every section of this sweep hardcoded theme: 'light', so a dark
  // regression has been invisible here for as long as the sweep has existed. Round 175 found the same gap in
  // the panel's fixture surfaces; this is the second home, and it is being done before it costs anything.
  // ONE WIDTH, not all three: 1440 is where the console is used, and three widths of dark would double a run
  // that already takes minutes for a difference that width does not create.
  for (const [label, hash] of (wants('dark') ? PAGES : [])) {
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
    // THE SAME WINDOW IN DARK, off the page that was just set to it. The panel's idle finding was a THEME-shaped
    // one once (the rail dot froze its paint across a flip), so the dark pass is not a formality here.
    if (wants('idle')) {
      const idle = await idlePass(page, 6000);
      report.idle.push({ page: label + '-dark', width: 1440, density: 'console', theme: 'dark', seconds: 6, ...idle });
    }
    // A FALSE POSITIVE WORTH REMEMBERING. While deciding whether this pass was needed I probed the page by
    // hand and got 4.3 for the active rail button — under AA, apparently a defect. The tested probe finds it
    // fine: the hand-rolled comparison read rgba(217, 72, 15, 0.9) as opaque and ignored what it composites
    // over, which is the ONE thing compositeStack exists to get right. The real maths is a few lines away in
    // lib/contrast-probe.mjs. Every exploratory probe of a colour should call it, because that is exactly the
    // moment the shortcut looks harmless.
    // HOVER IN DARK, on ONE page. The light hover pass found a dark-theme button at 1.94 when the PANEL
    // first ran it (round 84) — a hover colour that only exists in one theme is exactly what a
    // single-theme pass cannot see. One page rather than six: hover styles are per-class, the overview
    // carries the console's whole control vocabulary, and a full-DOM probe per hover per page would cost
    // six times what the question is worth.
    if (label === 'overview') {
      const all = await page.$$('button, [role="button"], a');
      const seenClass = new Set();
      const underAA = [];
      for (const h of all) {
        const key = await h.evaluate((el) => (typeof el.className === 'string' ? el.className : el.tagName));
        if (seenClass.has(key)) continue;
        seenClass.add(key);
        await h.hover();
        await page.waitForTimeout(40);
        for (const r of await page.evaluate(PROBE)) {
          if (r.kind !== 'graphic' && !r.inactive && r.cr < r.need) underAA.push(r.sel + ' ' + r.cr + '<' + r.need);
        }
      }
      // AND PUT THE POINTER BACK. Leaving the last hovered element under the cursor made the FOCUS pass that
      // follows measure a hovered control and report a ring that is not missing — a self-inflicted finding,
      // caught because the round's own report showed it. State must not leak between passes.
      await page.mouse.move(0, 0);
      report.hover.push({ page: 'overview-dark', width: 1440, density: 'console', theme: 'dark', interactive: all.length, underAA: [...new Set(underAA)] });
    }
  }
  await page.evaluate(() => { try { localStorage.setItem('vale-theme', 'light'); } catch (e) {} document.body.setAttribute('data-theme', 'light'); });

  // 320 IS IN THE LIST BECAUSE WCAG 1.4.10 NAMES IT. The criterion asks whether content reflows at 320 CSS
  // pixels — 400% zoom on a 1280 viewport — and this sweep tested 1440/900/720, so its "WCAG reflow" claim was
  // measured at a width the criterion does not mention. Round 224 measured the console at 640, 480 and 320
  // before adding it, and all three are CLEAN (docOver 0); the only inner overflow is pre.mt-8, a code block
  // with its own horizontal scroller, which is the case 1.4.10 exempts for content that needs two dimensions.
  // The panel was not so lucky at 640 (round 215), and that is the point: the width a check does not render is
  // the width where a real failure can sit unremarked.
  for (const width of (wants('reflow') ? [1440, 900, 720, 640, 320] : [1440])) {
    await page.setViewportSize({ width, height: 900 });
    for (const [label, hash] of PAGES) {
      await page.goto('https://ai.saisi.online/?cb=' + Date.now(), { waitUntil: 'load' });
      await page.evaluate((h) => { location.hash = h; }, hash);
      await page.waitForTimeout(1600);
      if (wants('contrast')) {
        const rows = await page.evaluate(PROBE);
        for (const r of rows) report.rows.push({ ...r, page: label, width, density: 'console', theme: 'light' });
        report.surfaces.push({ page: label, width, ...(await page.evaluate(SURFACE)) });
      }
      if (width === 1440) {
        // (a press-only run pays for this width and nothing else)
        if (wants('names')) report.names.push({ page: label, ...(await page.evaluate(NAMES)) });
        // Rendered classes with no matching rule — the mirror of dead CSS, and the failure a prune
        // causes. The browser's parsed selectors are the authority (rounds 79-80 removed 300+ lines
        // from this sheet). The styled count travels with the list as the tripwire. (No backticks in
        // here: this text is inside the emitted template, and the ninth stray one shut --emit down.)
        if (wants('unstyled')) report.unstyled.push({ page: label, ...(await page.evaluate(UNSTYLED)) });
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
        if (wants('focus')) report.focus.push(await focusPass(page, 16, { page: label, width }));
        // RENDERED PRESSES — the axis the panel has had since round 51 and this console had only at SHEET level.
        // feedback-check.mjs proves an :active RULE exists; it cannot see whether the press reaches the screen, and
        // round 54 had to fix ten console controls (the rail button, the avatar, the logout item, the icon button,
        // the language button, the auth tab, .btn-dashed, .card-link, .dev-mini and every link) by reading the sheet
        // alone. This measures them as the browser paints them. The pointer is moved OFF the element before release
        // so nothing is clicked; a target this page does not render is a NOTE, and the measured count is what keeps
        // a pass that pressed nothing from reading as clean. (No backticks in this comment: 43rd time.)
        const pressRows = wants('press') ? await pressPass(page, ['.rail-btn', '.btn', '.icon-btn', '.lang-btn', '.auth-tab', '.btn-dashed', '.card-link', '.dev-mini', '.rail-avatar', '.user-pop-logout'], { page: label, width }) : [];
        if (wants('press')) report.press.push({ density: 'console', theme: 'light', page: label, width, measured: pressRows.filter((r) => !r.note).length, rows: pressRows });
        // IDLE REPAINT, AND THE CONSOLE HAD NEVER BEEN MEASURED FOR IT (round 79). The panel got this pass in round
        // 64 and it found a live duration being called a repaint; the console polls its own views twice a second, so
        // "the page is settled and writing nothing" is exactly the claim its live views could break. It runs on
        // EVERY console page, in both themes, because this sweep has three pages and a six-second window each —
        // 36 seconds for the whole axis, which is cheaper than the panel's two densities and six pages make it.
        // The window lives inside the 1440 block where the other per-class passes are, because width changes what
        // is on screen and the idle question is about what a settled page does.
        if (wants('idle')) {
          const idle = await idlePass(page, 6000);
          report.idle.push({ page: label, width, density: 'console', theme: 'light', seconds: 6, ...idle });
        }

        // AND IT RUNS LAST, AFTER THE IDLE MEASUREMENT (round 192). This pass CLICKS — it has to, to see whether the
        // control answers — and its first placement sat before the idle window, so the presses' own state updates were
        // reported as "a repaint of unchanged output" on two console pages. pressPass never had that problem because it
        // moves the pointer OFF the element before releasing.
        // THE ACKNOWLEDGEMENT'S LATENCY, which this end had never measured (round 190). The press pass proves a press
        // PAINTS; this proves the control ANSWERS, and how fast, against the stated 100 ms budget. discover asks the DOM
        // for every visible control rather than a list somebody thought of — a list can only contain what somebody thought
        // of, and the controls that answer nothing are exactly the ones nobody thought about. Chrome is skipped: the rail
        // and the language button are navigation, not actions.
        // ACKPASS RETURNS THE ROWS THEMSELVES, not an object around them: the panel iterates its result directly, and
        // reading the artefact is what settled it after CI said "ackRows.rows is not iterable". Parsing is not running, and
        // the emitted script parsing was never evidence that this line worked.
        const ackRows = wants('ack') ? await ackPass(page, [], ACK_BUDGET_MS, {
          density: 'console', theme: 'light', page: label, mode: 'ack', discover: 8,
          skip: ['.rail-btn', '.lang-btn', '.avatar', 'a'],
        }) : [];
        if (wants('ack')) {
          // ONLY the ack rows go into report.ack: the shared judge reads that array, and the press array is judged by
          // different rules (a row of another shape there would be read as a press that measured nothing).
          report.ack = report.ack || [];
          for (const r of ackRows) report.ack.push(r);
        }
      }
    }
  }
  // REDUCED MOTION, both states, on the console's own overview page. The panel has had this since round
  // 134 and the console had nothing: the gap was recorded in round 136 when the pass was shared and left
  // unwired. render re-loads the page and re-applies the route, because the preference only takes
  // effect on a fresh style resolution.
  for (const width of (wants('motion') ? [1440] : [])) {
    await page.setViewportSize({ width, height: 900 });
    const render = async () => {
      await page.goto('https://ai.saisi.online/?cb=' + Date.now(), { waitUntil: 'load' });
      await page.evaluate((h) => { location.hash = h; }, '#/');
      await page.waitForTimeout(1400);
    };
    report.motion.push(await motionPass(page, render, { page: 'overview', width, density: 'console', theme: 'light' }));
  }

  // ANSWERED: THE RINGS WERE NEVER MISSING (round 186). Eighteen "missing focus rings" across six pages
  // were a FALSE POSITIVE of this sweep's own verdict, and six rounds went into a cascade that was never
  // broken. The proof is a screenshot: a keyboard-focused console button paints a 2px accent ring while
  // getComputedStyle reports "outline: solid 0px" and "box-shadow: none" for the very same element.
  //
  // Each wrong explanation was tested and killed by measurement, which is why this took six rounds rather
  // than one: a stale bundle (183), a cached sheet (184), a pointer leak left by the dark-hover pass (182),
  // a missing !important (185 — shipped, loaded, changed nothing), and a layered !important (186 — there
  // are no layers in the sheet at all). Along the way the device's assets directory was found holding EIGHT
  // files from four generations, which is what let each wrong explanation look right; it is pruned.
  //
  // THE LESSON IS THE ONE THIS SESSION KEEPS RELEARNING: a computed style is not a painted pixel. The
  // shared focusPass now treats its computed-style verdict as a CANDIDATE and confirms every no-ring
  // finding against the pixels — two small screenshots per candidate, so a clean page pays nothing — and
  // reports how many verdicts the pixels overruled.
  // TARGET SIZE, WCAG 2.5.8 — the check round 162 added for the PANEL, wired here because a check that
  // exists in one UI and not the others is the pattern this suite keeps paying for (rounds 135-136, 141).
  for (const [label, hash] of (wants('targets') ? [['overview', '#/'], ['devices', '#/devices']] : [])) {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('https://ai.saisi.online/?cb=' + Date.now(), { waitUntil: 'load' });
    await page.evaluate((h) => { location.hash = h; }, hash);
    await page.waitForTimeout(1500);
    report.targets.push({ page: label, ...(await page.evaluate(TARGETS)) });
  }

  // AND THE UNSTYLED CENSUS VISITS IT TOO (round 25). The rendered surfaces above visit the Overview with nothing
  // registered; the CENSUS walked only the six populated pages, so the one class this console declares
  // unstyled-by-design was still unseen and its note still asked for it — while the surfaces that DO carry it had
  // just been measured. A declaration is exercised by the pass that asks the question, not by a different one.
  if (wants('unstyled')) {
    empty.fleet = true;
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize({ width: 1440, height: 900 });
      await page.goto('https://ai.saisi.online/?cb=' + Date.now(), { waitUntil: 'load' });
      await page.evaluate((a) => {
        try { localStorage.setItem('vale-theme', a[0]); } catch (e) {}
        document.body.setAttribute('data-theme', a[0]);
        location.hash = a[1];
      }, [theme, '#/']);
      await page.waitForTimeout(1600);
      report.unstyled.push({ page: 'overview-empty' + (theme === 'dark' ? '-dark' : ''), ...(await page.evaluate(UNSTYLED)) });
    }
    empty.fleet = false;
  }

  // THE EMPTY FLEET, as surfaces of its own. Two pages have a meaningful empty form — Devices and Keys —
  // and neither had ever been rendered without content. Same recipe as the panel's empty state: a top-level
  // pass, one render each, recorded like any other surface.
  // BOTH THEMES, for the same reason the fixture surfaces took both in round 175: an empty state is mostly
  // COLOUR and TYPE, and a dark regression in one would be invisible to a light-only render.
  {
    empty.fleet = true;
    for (const theme of ['light', 'dark']) {
      // THE OVERVIEW JOINS THE EMPTY FLEET (round 25), and the reason is a measurement rather than symmetry. The
      // stat-off declaration in this sweep's implicitStates waives a class with NO matching rule — the Overview
      // builds a stat-card plus a stat-<tone> class, the sheet has rules for ok/warn/info only, and the unstyled pass
      // reported "1 of 1 declared unstyled-by-design class(es) were not seen in this run (0 unstyled name(s) over 6
      // pages)". The class fires when a tone is off, which happens on three of the Overview's four stats when
      // there is nothing to report: no device online, no channels, no keys. The empty-fleet fixture already exists
      // and visited only #/devices and #/keys, so the Overview in that state — and the faint bars the off tone
      // paints — had never been rendered by anything. A state with no surface cannot be measured; this is the
      // surface.
      for (const [label, hash] of [['overview-empty', '#/'], ['devices-empty', '#/devices'], ['keys-empty', '#/keys']]) {
        await page.setViewportSize({ width: 1440, height: 900 });
        await page.goto('https://ai.saisi.online/?cb=' + Date.now(), { waitUntil: 'load' });
        await page.evaluate((a) => {
          try { localStorage.setItem('vale-theme', a[0]); } catch (e) {}
          document.body.setAttribute('data-theme', a[0]);
          location.hash = a[1];
        }, [theme, hash]);
        await page.waitForTimeout(1600);
        const name = label + (theme === 'dark' ? '-dark' : '');
        report.themeChecks.push({ page: name, intended: theme, ...(await page.evaluate(THEME)) });
        const rows = await page.evaluate(PROBE);
        for (const r of rows) report.rows.push({ ...r, page: name, width: 1440, density: 'console', theme });
        report.surfaces.push({ page: name, width: 1440, ...(await page.evaluate(SURFACE)) });
        report.names.push({ page: name, ...(await page.evaluate(NAMES)) });
      }
    }
    empty.fleet = false;
  }

  // THE FAILURE STATE, for every page. The console's error surfaces — a card that could not load, a table with
  // nothing but a message — had never been rendered by anything, so their contrast, their type and their
  // states were unmeasured. One render per page with the flag up, recorded like any other surface.
  // BOTH THEMES. An error card is colour and type like any other state, and the operator's console may be dark
  // — a light-only render of it would leave exactly the regression this suite exists to catch.
  {
    fail.api = true;
    for (const theme of ['light', 'dark']) {
      for (const [label, hash] of PAGES) {
        await page.setViewportSize({ width: 1440, height: 900 });
        await page.goto('https://ai.saisi.online/?cb=' + Date.now(), { waitUntil: 'load' });
        await page.evaluate((a) => {
          try { localStorage.setItem('vale-theme', a[0]); } catch (e) {}
          document.body.setAttribute('data-theme', a[0]);
          location.hash = a[1];
        }, [theme, hash]);
        await page.waitForTimeout(1800);
        const name = label + '-fail' + (theme === 'dark' ? '-dark' : '');
        report.themeChecks.push({ page: name, intended: theme, ...(await page.evaluate(THEME)) });
        const rows = await page.evaluate(PROBE);
        for (const r of rows) report.rows.push({ ...r, page: name, width: 1440, density: 'console', theme });
        report.surfaces.push({ page: name, width: 1440, ...(await page.evaluate(SURFACE)) });
        report.names.push({ page: name, ...(await page.evaluate(NAMES)) });
      }
    }
    fail.api = false;
  }

  // THE LOGIN PAGE, IN BOTH THEMES. It is the one surface an operator sees before anything else works, and it
  // is a CARD — colour, type and a form — so a light-only render leaves a dark regression unmeasured. Round
  // 229 checked every other pass in all three sweeps for the same thing and found this as the only one that
  // both renders colour AND lacked a dark counterpart; the motion pass is light-only too and stays that way, because
  // it asks whether animations are DISARMED rather than what colour anything is.
  auth.signedIn = false;
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('https://ai.saisi.online/?cb=' + Date.now(), { waitUntil: 'load' });
    await page.evaluate((t) => {
      try { localStorage.setItem('vale-theme', t); } catch (e) {}
      document.body.setAttribute('data-theme', t);
    }, theme);
    await page.waitForTimeout(1600);
    const name = 'login' + (theme === 'dark' ? '-dark' : '');
    report.themeChecks.push({ page: name, intended: theme, ...(await page.evaluate(THEME)) });
    for (const r of await page.evaluate(PROBE)) report.rows.push({ ...r, page: name, width: 1440, density: 'console', theme });
    report.surfaces.push({ page: name, width: 1440, ...(await page.evaluate(SURFACE)) });
    report.names.push({ page: name, ...(await page.evaluate(NAMES)) });
  }
  await diag("done rows=" + (report.rows || []).length + " findings-source-ready pid=" + process.pid);
  fs.writeFileSync(REPORT_PATH, JSON.stringify(report));
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
  // THE STATES THIS SHEET DECLARES AND THIS RUN NEVER PAINTED (round 50 of the standing goal). The panel has had this
  // queue since round 32 — it is how `cmd-dot` was found rendering four of its six states, one of them with no rule at
  // all — and until now it lived inside the panel's sweep, so the console's unrendered states were SILENT. Same
  // implementation, this surface's sheets: the console's styles are SOURCE files (its dist is a pruned build artifact),
  // which is the same choice `console-marks-check.mjs` makes for the same reason.
  for (const line of markCoverageNotes(consoleSheets(), report)) console.log(line);
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
  const out = browserScript();
  assertEmbedded(out, ["focusPass", "pressDelta", "pressPass", "discoverPressTargets", "ackPass"]);
  process.stdout.write(out);
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
