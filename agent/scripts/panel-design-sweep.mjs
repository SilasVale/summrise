#!/usr/bin/env node
// panel-design-sweep — run every panel design check in one pass, and judge it.
//
// THE CHECKS ARE THE SHARED CORE'S (`lib/design-sweep.mjs`): headings, landmarks, overflow/clipping/
// slivers, accessible names, reflow, focus. This file adds only what is the PANEL's own — the
// harness it boots, the densities and themes, the approval mode, and boot/interaction timing — and
// its measurements are forwarded to the same judge every other UI uses.
//
// TWO MODES, because the browser lives on a device (the console MCP owns it) while the judging needs
// the tested maths that lives here:
//
//   node agent/scripts/panel-design-sweep.mjs --emit --passes=pages,hover > /tmp/sweep.js
//     writes the `browser_run_script` payload; run it on the device, save the JSON.
//   node agent/scripts/panel-design-sweep.mjs --judge <report.json> --expect=pages,hover
//     judges it, refusing a report that is missing any pass the caller said it wanted.
//
// RUN IT IN PASSES, and say which ones. The full sweep (pages, hover, motion, reflow, unstyled) grew
// past the caller's own timeout in round 90 — which made it a check that could not complete, i.e. one
// that would quietly stop running. `--passes` fixes that, and `--expect` is the half that matters: a
// partial run that found nothing reads exactly like a clean full one unless the caller declares what
// it asked for. Measured round 92: `--passes=unstyled` completes in seconds and reports 1123 styled
// classes per density; `--passes=pages` is the heavy one (1356 rows, 48 surfaces) and completes too.
//
// DEFAULT IS EVERYTHING, which still exceeds the timeout — the default is for the emit to stay
// honest, not to be run in one call.
//     judges that report and exits non-zero on any finding.
//
// The harness must exist on the device first: `node agent/scripts/panel-render-audit.mjs` emits it.
//
// MEASURED AND FOUND CLEAN (rounds 61-64), so the next round does not re-investigate:
//   * the chrome stack's vertical rhythm. Heights: evicted notice 26, goal bar 23, status bar 27 —
//     differences of 1-2px, below the threshold where a person can see them.
//   * the chrome stack's horizontal insets: notice text starts at 16px, status-bar text at 14px —
//     2px apart, same verdict.
//   * PADDINGS ARE NOT ON THE SPACING SCALE, and that is the codebase's norm rather than drift: 87
//     distinct off-scale paddings (5px, 7px, 9px, 11px, 13px, 14px, 22px…) across the sheet. Adding
//     a padding rule to the scale contract would flag ~87 values and mean a 1-2px rewrite of every
//     component — change nobody could verify as an improvement, so the rule was NOT added. The
//     properties that ARE contracted (radius, gap, type size) each had a handful of violations when
//     their rules were written, which is why those rules were worth having.
//
//   * THE HISTORY PAGE AT SCALE (round 69). The harness serves a populated archive with `?rows=N`,
//     which no sweep had ever done — every pass before it answered `/api/sessions` with a bare `{}`,
//     so the page had only ever been measured EMPTY. Measured with 50 / 200 / 800 recorded sessions:
//     the window is 50 rows, the DOM stays flat (395 nodes) whatever the archive holds, there are no
//     long tasks, and switching to the page costs 4-21ms. The page SAYS what it is doing — "showing
//     50 of 800" — so the count is honest, and the file's header documents what is deliberately NOT
//     here (no search over the archive, no paging). That is a boundary someone chose and wrote down,
//     not a defect: 750 sessions are unreachable from the UI, by decision.
//
//   * THE PAGES WITH CONTENT (rounds 69 and 78). The stub answers unknown `/api/*` with a bare `{}`,
//     so three surfaces had only ever been measured EMPTY or, worse, in an ERROR state: the History
//     page (fixed in 69 with `?rows=N`), the Plugins page (which rendered "inventory could not be
//     read" in EVERY sweep until round 78, because /api/spec and /api/plugins/status were never
//     served) and the Memory page (which reads through the tool route). All three are populated now,
//     and all three measure clean:
//         history   50-row window of 800, flat DOM, no long tasks
//         plugins   4 cards with tool counts, 132 rows, 0 under AA
//         memory    3 entries with tags, 82 rows, 0 under AA
//     The lesson is the one this file keeps re-learning: an unserved route is not an empty page, it
//     is an UNMEASURED page, and the difference is invisible in the results.
//
//   * THE MONITORS SURFACE, MEASURED AT LAST (round 101). It had never been rendered: `/api/monitors`
//     was served by nothing, so the card and the alert strip had only ever been seen empty. It is built
//     well. With a fixture in the device's real shape — `{ok: true, targets: […]}` and transitions of
//     `{at_ms, up, lasted_ms}` — the Settings card shows both targets with their `up`/`down` states, the
//     502, and 99 probe rows measuring clean. The ALERT STRIP is event-driven: it renders on the
//     device's `vale-monitor-change` push and nothing else, so a down target on screen with no push
//     shows no strip — which looks like a defect and is not one. Dispatched, it reads
//     "192.168.1.1:8000 is DOWN — it had been up 15m (HTTP 502)", carries role="status" and
//     aria-live="polite", and measures 15.31 light / 11.42 dark.
//
//   * THE TWO 1.16 tab-dot ROWS ARE AN INSTRUMENT ARTIFACT, ESTABLISHED BY REPRODUCTION (round 143).
//     Round 142's fresh 2012-row sweep returned three findings: two `span.tab-dot` rows at 1.16 in the
//     panel/dark density, `paint rgb(217,72,15) (background)` against the active tab's `rgb(198,67,16)`,
//     and one more. THE PAGE IS RIGHT: three separate renders of that exact state — the sweep's own
//     `?theme=dark&mode=idle&sessions=4` at both 1500ms and 2500ms — show the active dot carrying its ring,
//     `rgb(255, 255, 255) 0px 0px 0px 1px`, and an active tab present. So the row was captured in a state
//     the page does not reach at any timing I can reproduce.
//     WHAT TO DO IF IT RECURS: the row already carries `paint` and `surface` (which is why the no-ring
//     state is provable at all), but not WHETHER the element was inside `.active` when it was captured.
//     That field is the next step, and I tried to add it here and gave up after three failed edits to the
//     emitted-template escaping — the fix is a context field on the graphics row, not another reproduction.
//   * THE WHOLE PAGE SWEEP, ON A HARNESS THAT FINALLY HAS CONTENT (round 120). Six pages — Terminal,
//     History, Browser, Memory, Plugins, Settings — 48 surfaces, 1796 text nodes, 48 name checks, and
//     the judge returns ZERO findings with ZERO unmeasurable rows. The two div.tabrow overflow items are
//     the documented harness artifact and are printed as notes, not silently dropped. Before round 117
//     every card on those pages rendered empty, so a green sweep said much less than it looked like it did.
//
//   HOW TO RUN IT (reconstructed in round 120; it takes several steps and the next round should not have
//   to rediscover them):
//     1. locally:  node agent/scripts/panel-design-sweep.mjs --emit --passes=pages > /tmp/sweep-pages.js
//     2. upload it (curl -T to the relay) and system_file_download it to C:\ProgramData\Vale\pwout\
//     3. on the device, run it in-process — it drives the browser itself:
//          const code = fs.readFileSync(SRC, 'utf8');
//          new Function('require','module','exports','__dirname','__filename','process','console','Buffer',
//                       'setTimeout','clearTimeout', code)(require, {exports:{}}, {}, dir, SRC, process,
//                       console, Buffer, setTimeout, clearTimeout);
//        It prints its summary and rewrites C:\ProgramData\Vale\pwout\design-sweep.json (~390 KB).
//        NOTE: top-level await is NOT valid there — wrap any driver in an async IIFE.
//     4. upload that report, curl it down, and judge it locally with THIS adapter's waivers — a bare
//        judgeReport(report, {}) reports the div.tabrow artifacts as findings, which is what they are not.
//   * THE STATE TOKENS AS GRAPHICS, AUDITED (round 122). The probe measures TEXT, so a border or a mark
//     has no row at all — and the two real graphic defects this session found (the flapping chip's border,
//     the boot triangle) were both found BY HAND. Round 122 grepped every `color:`/`border*:` declaration
//     that uses a state FILL token (thirteen of them) and measured the ones that render:
//       .monitor-row[data-state=up]   border --state-ok   light 3.45   passes 3:1, margin thin
//       .monitor-row[data-state=down] border --state-warn light 5.02 / dark 3.20   PASSES, thinly
//       .monitor-mark.is-flapping     border --warn-ink   light 6.45 / dark 8.76   (round 109's fix)
//       .notify-state.is-denied       border --danger-on-soft  ~7.2                 passes
//     The down row was HARDENED to --warn-ink anyway (7.09 / 9.99): 3.20 passes, but it is the same fill
//     token that produced six real defects, and consistency here is cheaper than another hunt. Recorded as
//     a hardening, NOT as a fixed defect — it was passing.
//   * THE DOWN MONITOR STATE HAD STOPPED RENDERING (round 122): round 114 flipped the fixture's target up
//     to reach the flapping chip, and DOWN takes precedence, so the down row and its chip were gone and
//     nothing measured them. `?monitor=down` flips it back; the default still shows flapping.
//   * WHAT THE INSTRUMENT STILL CANNOT SEE: graphic contrast, at all. If a border or a mark is wrong, the
//     probe is silent and only a hand-written computed-style read finds it. That is the next instrument to
//     build, and until it exists this list is the only place these numbers live.
//
//   * THE "up" LABEL WAS 3.33 (round 118). With the Device area finally rendering, the probe caught
//     `span.monitor-state.up` at 3.33 in the light theme — `--state-ok`, the sixth time a state-dot FILL
//     token has been used as text (the flapping chip's --state-warn was the fifth). The panel already had
//     the right token: --success-text is #1e7a33 light / #69db7c dark. Re-measured on the rendered page:
//     5.22 light / 9.89 dark in the panel density, 5.36 / 9.39 on desktop. Its .down sibling always used
//     a text token (--danger-on-soft, 7.27), which is why only one half of the pair ever failed.
//   * THE `btn-ghost` "OPEN ITEM" FROM ROUND 118 WAS MINE, NOT THE PANEL'S (closed in round 119). The
//     element is "Notifications unavailable" with `disabled: true` and opacity 0.45 — an INACTIVE control,
//     which WCAG 1.4.3 exempts and which this suite has always waived: the probe sets `inactive`, the
//     judge filters on it, and the real sweep kept 41/41 green. The false item came from an ad-hoc reader
//     that filtered rows with `cr < need` and dropped the waiver. `contrast-probe.mjs` now says so at the
//     helpers: use `failures()`/`inactive()`, never a hand-rolled comparison.
//
//   * THE DEVICE AREA, POPULATED AT LAST (round 117). /api/status, /api/vitals/history and /api/boots
//     were answered by the catch-all {ok:true} in every sweep this harness has ever produced, so the
//     Device health card had only ever been measured EMPTY. With real fixtures it reads:
//       "No sustained load in this window: CPU high 38%, memory high 44%. CPU avg 26% low 8% high 38%
//        Mem avg 42% low 38% high 44% 16.0 GB 40 readings over 20m - one every 30s"
//     - four sparklines at 132x28, the span arithmetic correct for 40 samples at 30 s, the memory total
//     showing 16.0 GB, the boot verdict "crashed" from last_boot_kind, and the two monitor rows beside
//     it. Both densities and both themes, no page errors. The CARD also carries a verdict sentence
//     ("No sustained load in this window") rather than only numbers, which is the design touch worth
//     keeping when this area is next touched.
//
//   * THE MONITOR CHIPS, BOTH STATES (round 109). The harness fixture now produces a FLAPPING target
//     (up now, 5 drops — the device's own UNSTABLE_DROPS is 2), because the component gives DOWN
//     precedence: `if (down.length === 0) { …flapping… }`, so a down target anywhere hides the flapping
//     chip entirely and the two NEVER appear together. That precedence is a state property nobody had
//     rendered, and it is why the flapping chip's dark-theme reading went unseen: the flapping chip
//     measured 4.57 light / 2.80 dark, its colour (--state-warn) being a FILL token that does not flip
//     while the chip's surface does. Fixed to --warn-ink and re-measured on the rendered chip: 6.45
//     light / 9.13 panel-dark / 8.23 desktop-dark. The DOWN chip was measured in the same round and was
//     always fine — 6.84 light / 6.34 dark, from a token that flips.
//
// WHAT IT CANNOT SEE, stated so nobody trusts it further than it goes:
//   * anything inside the Electron shell — the evidence drawer and the embedded browser pane mount
//     only behind `window.valeEmbedded`, so a plain-browser harness renders an explanation page
//     (measured, round 45);
//   * the panel-density tab strip's own width: `#tabs` measures ~0px in the harness and 211px on the
//     device, so harness geometry findings pointing at tab children are suspect;
//   * a background that is an image (judged by the worst-BASE rule in the probe suite instead);
//   * any state the harness fixture cannot produce. Add the fixture, or say the state is unmeasured.
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { failures, unmeasurable, PROBE_SOURCE } from "./lib/contrast-probe.mjs";
import { markCoverageNotes, pageChecks, judgeReport, reportSummary, UNSTYLED_SOURCE, focusPass, motionPass, pressPass, discoverPressTargets, revealPass, ackPass, idlePass, assertEmbedded, TARGETS_SOURCE, THEME_SOURCE, DIAG_SOURCE, pressDelta } from "./lib/design-sweep.mjs";

const mode = process.argv[2];
/** `--passes=pages,hover` limits the emitted script; the default is everything. Recorded in the report
 *  so the judge can refuse a partial one — the same rule as the audit's exit codes: a run that did not
 *  measure something must not look like a run that measured it and found nothing. */
const PASSES = (process.argv.find((a) => a.startsWith("--passes=")) || "--passes=all").slice("--passes=".length);

/** The panel's own extra: what still animates when the user has asked for less motion. */
const MOTION = `
const MOTION = \`(() => {
  const animating = [];
  for (const el of document.querySelectorAll('#root *')) {
    const st = getComputedStyle(el);
    const dur = parseFloat(st.transitionDuration) > 0 ? st.transitionDuration : null;
    const anim = st.animationName && st.animationName !== 'none' ? st.animationName + ' x' + st.animationIterationCount : null;
    if (!dur && !anim) continue;
    const key = typeof el.className === 'string' && el.className ? '.' + el.className.trim().split(/\\\\s+/).join('.') : el.tagName.toLowerCase();
    animating.push(key + (dur ? ' trans=' + dur : '') + (anim ? ' anim=' + anim : ''));
  }
  return { animating: [...new Set(animating)] };
})()\`;
`;

/** The panel's own extra: boot and interaction timing (the other UIs do not measure it). */
const TIMING = `
const TIMING = \`(() => {
  const nav = performance.getEntriesByType('navigation')[0] || {};
  const paints = {};
  for (const p of performance.getEntriesByType('paint')) paints[p.name] = Math.round(p.startTime);
  return { firstPaintMs: paints['first-contentful-paint'] || null, domContentLoadedMs: Math.round(nav.domContentLoadedEventEnd || 0), nodes: document.querySelectorAll('#root *').length };
})()\`;
`;

// COMPUTED AT EMIT TIME, not baked once by hand. Round 190 wrote this as a LITERAL, so it only ever matched
// the stylesheet that happened to exist the day it was written: every rebuild afterwards made the guard fire
// on a perfectly current harness, and round 210 hit exactly that. The value is computed HERE, in the module,
// and only its result is inlined into the emitted script — the computation itself uses import.meta, which is
// a syntax error in the CommonJS script the device runs. (The emitter's own parse guard caught that, which is
// what it is for.)
const HARNESS_STAMP = (() => {
  try {
    const css = readFileSync(new URL("../resources/panel/panel.css", import.meta.url));
    return css.length + "-" + createHash("sha256").update(css).digest("hex").slice(0, 12);
  } catch (e) { return "(unreadable)"; }
})();
function browserScript() {
  const script = `const fs = require('fs');
// WHERE IT READS AND WRITES IS OVERRIDABLE, so the same sweep can run on the device (the defaults, exactly
// as before) or on any machine with a browser — which is what makes a CI job possible at all. Round 204:
// the design suite has only ever run when the loop remembered to run it, and a measured-and-verified
// objective should not depend on that. Nothing else about a device run changes.
const HARNESS = process.env.VALE_PANEL_HARNESS || 'C:\\\\ProgramData\\\\Vale\\\\pwout\\\\panel-harness.html';
const REPORT_PATH = process.env.VALE_SWEEP_REPORT || 'C:\\\\ProgramData\\\\Vale\\\\pwout\\\\design-sweep.json';
// THE BUILD THIS SWEEP WAS EMITTED AGAINST. The audit stamps every harness with the stylesheet it
// inlined; baking the expectation here turns round 189's note into a check. A delivered fixture that
// predates a CSS fix otherwise reports findings that look live — a 17px tab strip against a 962px build —
// and nothing in the report distinguishes them from a regression.
const EXPECTED_HARNESS_BUILD = ${JSON.stringify(HARNESS_STAMP)};
const PROBE = ${JSON.stringify(PROBE_SOURCE)};
${pageChecks("#root")}
const UNSTYLED = ${JSON.stringify(UNSTYLED_SOURCE)};
const TARGETS = ${JSON.stringify(TARGETS_SOURCE)};
const THEME = ${JSON.stringify(THEME_SOURCE)};
// THE SWEEP REPORTS ITSELF TO THE AGENT'S DIAGNOSTIC RING, so a caller whose tool call timed out can tell
// a run that is still working from one that was killed (round 181 lost half an hour to exactly that).
${DIAG_SOURCE}
const focusPass = ${focusPass.toString()};
const pressDelta = ${pressDelta.toString()};
const pressPass = ${pressPass.toString()};
// AND EVERY HELPER THAT FUNCTION CALLS. pressPass asks the DOM for the page's controls, and a version of this
// shipped WITHOUT this line: the emitted sweep called discoverPressTargets, the definition was not in the file, and
// the run died with "FATAL discoverPressTargets is not defined" — in CI, because no local gate RUNS the artifact
// (they plant defects in a report and judge it). The emitter checks its own output for exactly this now.
const discoverPressTargets = ${discoverPressTargets.toString()};
const revealPass = ${revealPass.toString()};
const ackPass = ${ackPass.toString()};
const idlePass = ${idlePass.toString()};
const motionPass = ${motionPass.toString()};
${MOTION}
${TIMING}
(async () => {
  const { acquireBrowser } = require(process.env.VALE_BROWSER_HELPER);
  const { page, close } = await acquireBrowser();
  const html = fs.readFileSync(HARNESS, 'utf8');
  // WHICH GENERATION OF THE HARNESS IS BEING MEASURED, in the report. The stamp is written by the audit that
  // generates the file; without it a delivered copy that predates a CSS fix reports findings that look real
  // (round 189: a 17px tab strip against a 962px build) and nothing distinguishes them from a live defect.
  const harnessBuild = (/<meta name="vale-harness-build" content="([^"]+)"/.exec(html) || [])[1] || "(unstamped — an older generation)";
  // A STALE FIXTURE INVALIDATES EVERY MEASUREMENT BELOW IT, so this is a finding rather than a note. The
  // stamp is unknown only for harnesses generated before round 189, which are stale by definition.
  const harnessStale = harnessBuild !== EXPECTED_HARNESS_BUILD;
  const stamp = Date.now();
  await page.route('http://vale.test/**', (route) =>
    route.fulfill({ status: 200, contentType: 'text/html; charset=utf-8', headers: { 'cache-control': 'no-store' }, body: html }));

  await diag("start focus,pages pid=" + process.pid);
  const report = {
    harnessBuild,
    expectedHarnessBuild: EXPECTED_HARNESS_BUILD,
    ...(harnessStale ? { harnessStale: true } : {}),
    // WHICH PASSES RAN, recorded in the report itself. The sweep outgrew its caller's timeout in round
    // 90 (a check that cannot complete is a check that will quietly stop running), so passes are
    // selectable — and a PARTIAL report must not read as a clean one, which is why this list travels
    // with the data and the judge refuses a report that does not say it covered everything.
    passes: ${JSON.stringify(PASSES)},
    rows: [], surfaces: [], reflow: [], names: [], timing: [], focus: [], motion: [], hover: [], unstyled: [], targets: [], themeChecks: [], sse: [],
  };
  // The harness publishes window.__sse (opened, fail). Read as data, judged by the shared clause.
  const SSE = "(() => window.__sse || null)()";
  const wants = (name) => report.passes === "all" || report.passes.split(",").map((p) => p.trim()).includes(name);
  for (const [density, path_, vp] of [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]]) {
    for (const theme of ['light', 'dark']) {
      // THE PAGE MUST BE RENDERED FOR *EITHER* PASS. The focus block below lives in this loop, so
      // --passes=focus used to run it ZERO times: the mode list was empty, no page was loaded, no Tab
      // was pressed, and the report came back focus: [] — clean, and clean because nothing ran. The
      // full sweep hid it, since all includes pages. Same defect as the ones this suite keeps
      // finding in its own checks, this time in the wiring between two of them.
      const needsPage = wants("pages") || wants("focus") || wants("timing") || wants("hover") || wants("press") || wants("idle");
      for (const mode_ of needsPage ? (wants("pages") ? ['idle', 'relaxed'] : ['idle']) : []) {
        await page.setViewportSize(vp);
        const t0 = Date.now();
        await page.goto('http://vale.test' + path_ + '?theme=' + theme + '&mode=' + mode_ + '&sessions=4&cb=' + stamp, { waitUntil: 'load' });
        await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
        await page.reload({ waitUntil: 'load' });
        await page.waitForSelector('.side-row, .dtab, .tab', { timeout: 20000 });
        const toFirstRow = Date.now() - t0;
        await page.waitForTimeout(1200);
        report.timing.push({ density, theme, mode: mode_, toFirstRowMs: toFirstRow, ...(await page.evaluate(TIMING)) });

        // FOCUS RINGS BY REAL TAB PRESSES, in ONE implementation shared with the console and the
        // extension (lib/design-sweep.mjs). It was copied between adapters once and the copies
        // drifted for two rounds; the loop lives in the core now.
        report.focus.push(await focusPass(page, 14, { density, theme }));

        // THE PRESS, ON THE SAME PAGE, RIGHT AFTER THE FOCUS PASS (round 55). It is the half feedback-check.mjs
        // cannot do: that gate proves an :active RULE EXISTS, and round 51 found the ACTIVE TAB dead with the rule
        // sitting right there in the sheet. Targets differ per density — the panel renders .tab where the desktop
        // renders .dtab — and one that is absent is a NOTE, not a finding, because the two surfaces do not carry
        // the same controls. The measured count is what keeps that from becoming a pass that presses nothing.
        // (No backticks in this comment: it lives inside the emitted template literal, and one ends it — 39th time.)
        // IDLE REPAINT (round 64): the page is settled, the fixtures are static, and the panel should be writing
        // nothing at all. Anything it does write is a clock or a recomputation from unchanged inputs.
        // ONE PAGE, not six: the measurement is a SIX-SECOND window, and running it on every page of every density
        // and theme would spend two and a half minutes proving the same thing. Terminal is the panel's default page.
        if (wants("idle")) {
          const idle = await idlePass(page, 6000);
          report.idle = report.idle || [];
          // This is the page the mode loop just loaded — the default one, Terminal — because the rail walk that
          // names the other pages happens further down. Calling it label here was the first version's bug: there is
          // no such binding in this scope, and the device answered "label is not defined". (44th backtick.)
          report.idle.push({ density, theme, mode: mode_, page: density + "-Terminal", seconds: 6, ...idle });
        }
        if (wants("press")) {
          const pressTargets = ['.rail-btn', '.desktop-rail-btn', '.tab', '.dtab', '.side-row', '.side-add'];
          const pressRows = await pressPass(page, pressTargets, { density, theme, mode: mode_ });
          report.press = report.press || [];
          report.press.push({ density, theme, mode: mode_, measured: pressRows.filter((r) => !r.note).length, rows: pressRows });
        }

        const rail = await page.evaluate(() => {
          const r = document.querySelector('#icon-rail, .desktop-rail');
          return r ? [...r.querySelectorAll('button')].map((b) => (b.getAttribute('aria-label') || b.title || '').trim()).filter((l) => l && !/theme|started/i.test(l)) : [];
        });
        for (const label of rail) {
          await page.evaluate((l) => {
            const r = document.querySelector('#icon-rail, .desktop-rail');
            const b = [...(r ? r.querySelectorAll('button') : [])].find((x) => (x.getAttribute('aria-label') || x.title || '').trim() === l);
            if (b) b.click();
          }, label);
          await page.waitForTimeout(450);
          const rows = await page.evaluate(PROBE);
          for (const row of rows) report.rows.push({ ...row, density, theme, mode: mode_, page: label });
          report.surfaces.push({ density, theme, mode: mode_, page: density + '-' + label, ...(await page.evaluate(SURFACE)) });
          report.names.push({ density, theme, mode: mode_, page: density + '-' + label, ...(await page.evaluate(NAMES)) });
          // DID THE HARNESS DELIVER THE PUSH? The panel's connected state comes from a complete frame on
          // /api/events/term, and the fixture that serves it is the only thing that knows whether it was served.
          // This was a NOTE in the harness for many rounds ("with the stream shut, every panel measurement this
          // harness has ever produced was taken in a reconnecting state") and nothing asserted it, so a change that
          // broke the stream would have gone on being measured as if it were the product.
          report.sse.push({ density, theme, mode: mode_, page: density + '-' + label, ...(await page.evaluate(SSE)) });
        }
      }
    }
  }

  // THE EMPTY STATE, desktop density — the only one of the two that can be measured here.
  //
  // Round 152 added this for the PANEL density and measured it "clean". Round 153 looked at what that
  // surface actually rendered and found it was not the empty state at all: the rail said "Sessions
  // unavailable — reconnecting…", because in the panel harness the rail receives connected=false, while the
  // DESKTOP harness renders the real thing ("No sessions yet"). The contrast was clean in both cases, which
  // is exactly why the label mattered — a surface that measures the wrong state passes for the best reason.
  // The panel block was PRUNED rather than left in place with a caveat.
  //
  // One render, top level, no nesting arithmetic: this is the state a fresh install sees.
  // BOTH THEMES. These fixture surfaces carry COLOUR, and colour is the one thing a theme changes — measuring
  // them in light alone leaves a dark regression invisible, which is the gap rounds 148-161 spent their time
  // closing everywhere else. (The target-size and type-floor passes stay light-only on purpose: a box and a
  // font size do not change with the theme.)
  for (const theme of wants("pages") ? ['light', 'dark'] : []) {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('http://vale.test/desktop/?theme=' + theme + '&mode=relaxed&sessions=0&cb=' + stamp, { waitUntil: 'load' });
    await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
    await page.reload({ waitUntil: 'load' });
    await page.waitForTimeout(2000);
    const rows = await page.evaluate(PROBE);
    for (const row of rows) report.rows.push({ ...row, density: 'desktop', theme, mode: 'relaxed', page: 'Desktop-empty' });
    report.themeChecks.push({ page: 'Desktop-empty', intended: theme, ...(await page.evaluate(THEME)) });
    report.surfaces.push({ density: 'desktop', theme, mode: 'relaxed', page: 'Desktop-empty', ...(await page.evaluate(SURFACE)) });
    report.names.push({ density: 'desktop', theme, mode: 'relaxed', page: 'Desktop-empty', ...(await page.evaluate(NAMES)) });
  }

  // THE NEW-SESSION MENU, WHICH ONLY A CLICK CAN RENDER (round 92). DesktopShell holds it: a .btn-new button with
  // aria-expanded, and a popover of role=menuitem buttons, each with an .nm-ico span carrying data-kind. The sweep
  // has PRESSED that button for many rounds — .btn-new is in the press targets — and never once photographed what
  // it opens, so the menu's item contrast, its target sizes and the per-kind icon colours have been unmeasured since
  // the menu replaced four buttons with one entry point. Desktop only, because the menu is: the panel density has no
  // .desktop-new at all.
  //
  // The entrance animation is new-menu-in (declared ATTENTION in chrome-stillness-check), so the wait after the
  // click is longer than the press pass's: this photographs the SETTLED menu, not its first frame.
  for (const theme of wants("pages") ? ['light', 'dark'] : []) {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('http://vale.test/desktop/?theme=' + theme + '&mode=idle&sessions=4&cb=' + stamp, { waitUntil: 'load' });
    await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
    await page.reload({ waitUntil: 'load' });
    await page.waitForTimeout(1800);
    await page.evaluate(() => { const b = document.querySelector('.btn-new'); if (b) b.click(); });
    await page.waitForTimeout(700);
    const pname = 'Desktop-NewMenu-' + theme;
    const rows = await page.evaluate(PROBE);
    for (const row of rows) report.rows.push({ ...row, density: 'desktop', theme, mode: 'menu', page: pname });
    report.surfaces.push({ density: 'desktop', theme, mode: 'menu', page: pname, ...(await page.evaluate(SURFACE)) });
    report.names.push({ density: 'desktop', theme, mode: 'menu', page: pname, ...(await page.evaluate(NAMES)) });
    report.sse.push({ density: 'desktop', theme, mode: 'menu', page: pname, ...(await page.evaluate(SSE)) });
  }

  // THE PANEL DENSITY'S EMPTY STATE, RE-ADDED BECAUSE THE REASON IT WAS PRUNED HAS EXPIRED (round 91).
  //
  // Round 152 added this surface for the panel density and measured it "clean". Round 153 looked at what it had
  // actually rendered and found it was NOT the empty state: the rail said "Sessions unavailable — reconnecting…",
  // because in that harness the rail received connected=false. The desktop harness rendered the real thing ("No
  // sessions yet"); the panel block was PRUNED rather than left in place with a caveat, and the note kept the
  // sentence worth remembering: "a surface that measures the wrong state passes for the best reason."
  //
  // THE PREMISE HAS EXPIRED. Rounds 86 and 87 established — and CI now asserts on every run — that this harness DOES
  // open the SSE stream: the panel renders connected, 4142 text nodes contain no "Sessions unavailable" on a surface
  // whose harness did not report the failure fixture, and the judge FAILS a surface that regresses. So the block can
  // come back, and the clause that proved the harness connected is also what makes it safe to re-add: if the panel
  // renders the wrong state here again, THIS SURFACE fails instead of passing.
  for (const theme of wants("pages") ? ['light', 'dark'] : []) {
    await page.setViewportSize({ width: 1280, height: 860 });
    await page.goto('http://vale.test/panel/?theme=' + theme + '&mode=relaxed&sessions=0&cb=' + stamp, { waitUntil: 'load' });
    await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
    await page.reload({ waitUntil: 'load' });
    await page.waitForTimeout(2000);
    const rows = await page.evaluate(PROBE);
    for (const row of rows) report.rows.push({ ...row, density: 'panel', theme, mode: 'relaxed', page: 'Panel-empty' });
    report.themeChecks.push({ page: 'Panel-empty', intended: theme, ...(await page.evaluate(THEME)) });
    report.surfaces.push({ density: 'panel', theme, mode: 'relaxed', page: 'Panel-empty', ...(await page.evaluate(SURFACE)) });
    report.names.push({ density: 'panel', theme, mode: 'relaxed', page: 'Panel-empty', ...(await page.evaluate(NAMES)) });
    // THE FLAG TRAVELS WITH THIS ONE, unlike the desktop block above: it is the whole reason the surface can exist.
    report.sse.push({ density: 'panel', theme, mode: 'relaxed', page: 'Panel-empty', ...(await page.evaluate(SSE)) });
  }

  // THE UPDATE CARD MID-RELEASE. ?busy=1 exists in the harness and NO SWEEP HAS EVER RENDERED IT: the mode
  // list is idle/relaxed, so the state an operator stares at while a release is running has been measured
  // zero times. Measured by hand first (113 rows, no failing text, and the one disabled control is the
  // Notifications button at opacity 0.45, which the probe marks inactive and WCAG exempts). One render of
  // the Settings page, where the card lives, on the same top-level recipe as the empty state.
  // BOTH DENSITIES. The desktop shell is what the operator actually uses, and it renders DIFFERENT markup
  // from the panel (dtab vs tab, a header row instead of a canvas top), so a defect in its busy card would
  // be invisible to a panel-only render. The rail lookup below already handled both rails — the desktop
  // render was simply never asked for.
  for (const [density, path_, vp] of wants("pages") ? [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]] : []) {
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize(vp);
    await page.goto('http://vale.test' + path_ + '?theme=' + theme + '&mode=idle&sessions=3&busy=1&cb=' + stamp, { waitUntil: 'load' });
    await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
    await page.reload({ waitUntil: 'load' });
    await page.waitForTimeout(1800);
    await page.evaluate(() => {
      const rail = document.querySelector('#icon-rail, .desktop-rail');
      const b = [...(rail ? rail.querySelectorAll('button') : [])].find((x) => /setting/i.test((x.getAttribute('aria-label') || '') + (x.textContent || '')));
      if (b) b.click();
    });
    await page.waitForTimeout(1400);
    const rows = await page.evaluate(PROBE);
    const name = (density === 'desktop' ? 'Desktop-settings-busy' : 'Settings-busy');
    for (const row of rows) report.rows.push({ ...row, density, theme, mode: 'busy', page: name });
    report.themeChecks.push({ page: name, intended: theme, ...(await page.evaluate(THEME)) });
    report.surfaces.push({ density, theme, mode: 'busy', page: name, ...(await page.evaluate(SURFACE)) });
    report.names.push({ density, theme, mode: 'busy', page: name, ...(await page.evaluate(NAMES)) });
  }
  }

  // EVERY RAIL PAGE, IN BOTH DENSITIES (round 41 of the standing goal). Every surface above renders the page the
  // app opens on, plus Settings — so the OTHER SIX rail pages were measured by nothing, in EITHER density, and
  // round 40 found a two-loud reading on one of them by probing them by hand. This walks the rail instead of the
  // URL: click each button, read back WHICH button is now active, and name the surface after it.
  //
  // THE READ-BACK IS THE POINT. A click that silently fails would measure the SAME page eight times and report
  // eight clean surfaces, which is the "a scan that read nothing is not a clean scan" trap wearing a progress bar.
  // Naming each surface by the label the app itself reports means a failed click shows up as a DUPLICATE page name
  // in the report rather than as coverage that is not there.
  for (const [density, path_, vp] of wants("pages") ? [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]] : []) {
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize(vp);
      await page.goto('http://vale.test' + path_ + '?theme=' + theme + '&mode=idle&sessions=4&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(2000);
      const buttons = await page.evaluate(() => [...document.querySelectorAll('#icon-rail button, .desktop-rail button')].map((b) => (b.getAttribute('aria-label') || b.textContent || '').trim().slice(0, 18)));
      const readActive = () => page.evaluate(() => {
        const on = document.querySelector('#icon-rail button.active, .desktop-rail button.active') || document.querySelector('#icon-rail button[aria-current], .desktop-rail button[aria-current]');
        return on ? (on.getAttribute('aria-label') || on.textContent || '').trim().slice(0, 18) : '';
      });
      let previous = '';
      let pages = 0;
      // THE THEME THIS WALK STARTS FROM, so the undo below has something to compare against.
      const before = (await page.evaluate(THEME)).attr;
      for (let i = 0; i < buttons.length; i++) {
        await page.evaluate((k) => { const b = document.querySelectorAll('#icon-rail button, .desktop-rail button')[k]; if (b) b.click(); }, i);
        await page.waitForTimeout(900);
        const active = await readActive();
        // NOT EVERY RAIL BUTTON IS A PAGE. Measured (round 41): the rail holds EIGHT buttons and only SIX are pages —
        // the seventh is the THEME TOGGLE and the eighth opens the getting-started guide. Clicking the toggle flips
        // the theme for every surface after it, which is how a run can label a DARK page as light and mean it. So a
        // click that does not move the active button is UNDONE and skipped: it is an action, not a destination.
        if (active === previous || !active) {
          await page.evaluate((k) => { const b = document.querySelectorAll('#icon-rail button, .desktop-rail button')[k]; if (b) b.click(); }, i);
          await page.waitForTimeout(600);
          // AND THE UNDO IS CHECKED. Round 41 clicked the action a second time to undo it and MOVED ON; if that click
          // lands on something that is not a toggle, or the app ignores it, every surface after it is rendered in the
          // other theme while this loop keeps saying the one it intended — which is exactly what CI reported in round
          // 73: six pages labelled light, rendered dark, with the light theme's warning ink on them.
          const after = (await page.evaluate(THEME)).attr;
          if (before && after && after !== before) {
            await page.evaluate((k) => { const b = document.querySelectorAll('#icon-rail button, .desktop-rail button')[k]; if (b) b.click(); }, i);
            await page.waitForTimeout(600);
            const back = (await page.evaluate(THEME)).attr;
            if (back && back !== before) report.railThemeDrift = (report.railThemeDrift || 0) + 1;
          }
          continue;
        }
        previous = active;
        pages++;
        const name = density + '-' + active;
        // THE LABEL COMES FROM THE PAGE, NOT FROM THE LOOP (round 74). The loop's own theme value is what it
        // INTENDED; the document's data-theme attribute is what it IS. They can disagree — that is the whole subject
        // of the theme-lie axis — and when they do, labelling the rows with the intention is how six contrast
        // findings were filed against a theme that was not on screen. One reading, used for every label, and the
        // disagreement is still recorded against the intention so the axis keeps its evidence. (No backticks: this
        // comment is inside the emitted template literal — 45th time.)
        const themeRead = await page.evaluate(THEME);
        const pageTheme = themeRead.attr === 'dark' ? 'dark' : themeRead.attr === 'light' ? 'light' : theme;
        const rows = await page.evaluate(PROBE);
        for (const row of rows) report.rows.push({ ...row, density, theme: pageTheme, mode: 'rail', page: name });
        report.themeChecks.push({ page: name, intended: theme, ...themeRead });
        report.surfaces.push({ density, theme: pageTheme, mode: 'rail', page: name, ...(await page.evaluate(SURFACE)) });
        report.names.push({ density, theme: pageTheme, mode: 'rail', page: name, ...(await page.evaluate(NAMES)) });
        // AND THE STATE A HOVER REVEALS, ASKED FOR RATHER THAN STUMBLED INTO (round 16). .side-actions is
        // display:none until the row is hovered, so the target probe has always read 0x0 and skipped it; the one
        // time it was measured, the press pass happened to leave the pointer on a row. This hovers a row, measures,
        // and parks the pointer again.
        if (wants("targets")) {
          const revealed = await revealPass(page, '.side-row', TARGETS, { density, theme: pageTheme, mode: 'reveal', page: name });
          if (revealed) report.targets.push(revealed);
        }
        // AND THE CONTROLS THIS PAGE HAS, WHICH NO LIST NAMED (round 15). The press pass ran on the Terminal surfaces
        // against a CURATED list, so a control on any other page had never been pressed: .device-logs-toggle — the
        // button that opens a log file's tail — had cursor:pointer and NO hover and NO press, and feedback-check
        // cannot see that shape (it demands a press only where a HOVER exists). The DOM is asked instead, deduped by
        // class+size and CAPPED, and the count travels into the report so the judge sizes its floor to THIS page:
        // the Browser page renders an explanation with one control, and one pressed is a complete pass there.
        if (wants("press")) {
          const pressed = await pressPass(page, [], {
            density, theme: pageTheme, mode: 'rail', page: name, discover: 16,
            // WHAT ANOTHER PASS ALREADY PRESSES: the mode passes own the rail buttons, the session tabs and the side
            // rows. Skipping them here is what lets the cap reach the page's OWN controls — the log toggles, the
            // archive rows, the view switches — which is the whole reason this pass exists.
            skip: ['.rail-btn', '.desktop-rail-btn', '.tab', '.dtab', '.side-row', '.side-add'],
          });
          report.press = report.press || [];
          report.press.push({
            density, theme: pageTheme, mode: 'rail', page: name,
            found: pressed.find((r) => r.found != null)?.found ?? null,
            measured: pressed.filter((r) => !r.note && r.changed).length,
            rows: pressed,
          });
        }
      }
      // THE COVERAGE IS WHAT CHANGED, not what was clicked: six pages is the fact, and a report that says fewer
      // means the rail stopped navigating rather than that the pages are clean.
      report.railPages = (report.railPages || 0) + pages;
    }
  }

  // SIXTEEN SESSIONS, IN BOTH DENSITIES — the state the operator actually complained about. Rounds 169-172
  // fixed ten identical pwsh labels, added the +N chip, moved the view switch out of the strip and cured a
  // strip that had collapsed to 35px. EVERY SURFACE IN THIS SUITE RENDERS 0, 3 OR 4 SESSIONS, so none of that
  // work has ever been drawn by a measuring run: a regression in the chip, or in the label disambiguation, or
  // in the desktop strip's separate renderer, would be invisible. 16 is the operator's own count.
  for (const [density, path_, vp] of wants("pages") ? [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]] : []) {
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize(vp);
      await page.goto('http://vale.test' + path_ + '?theme=' + theme + '&mode=idle&sessions=16&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(2000);
      const rows = await page.evaluate(PROBE);
      const name = (density === 'desktop' ? 'Desktop-16-sessions' : 'Terminal-16-sessions');
      for (const row of rows) report.rows.push({ ...row, density, theme, mode: 'overflow', page: name });
      report.themeChecks.push({ page: name, intended: theme, ...(await page.evaluate(THEME)) });
      report.surfaces.push({ density, theme, mode: 'overflow', page: name, ...(await page.evaluate(SURFACE)) });
      report.names.push({ density, theme, mode: 'overflow', page: name, ...(await page.evaluate(NAMES)) });
    }
  }

  // THE TWO REMAINING FIXTURE STATES, both hand-measured in earlier rounds and swept by nothing. Round 158
  // closed this gap for the message tones and round 159 for the busy card; these are the last two the
  // harness can express.
  //
  //   ?fail=1        every device call fails, so the cards render their ERROR surfaces. This is what the
  //                  operator sees when the device is unreachable, and no sweep has ever rendered it.
  //   ?monitor=down  a monitor target that is down, which flips a chip and the alert strip.
  //
  // Same top-level recipe as the empty and busy surfaces. (No backticks in this comment: 23 rounds have
  // paid for that lesson and the emitter now refuses to ship one.)
  if (wants("pages")) {
    for (const [density, path_, vp] of [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]])
    for (const [page_, query, lands] of [
      ...['light', 'dark'].map((t) => ['Terminal-fail-' + t, '?theme=' + t + '&mode=idle&sessions=3&fail=1', 'Terminal']),
      ...['light', 'dark'].map((t) => ['Settings-monitor-down-' + t, '?theme=' + t + '&mode=idle&sessions=3&monitor=down', 'Settings']),
    ]) {
      await page.setViewportSize(vp);
      await page.goto('http://vale.test' + path_ + query + '&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(1800);
      await page.evaluate((want) => {
        if (want === 'Terminal') return;
        const rail = document.querySelector('#icon-rail, .desktop-rail');
        const b = [...(rail ? rail.querySelectorAll('button') : [])].find((x) => new RegExp(want, 'i').test((x.getAttribute('aria-label') || '') + (x.textContent || '')));
        if (b) b.click();
      }, lands);
      await page.waitForTimeout(1400);
      const rows = await page.evaluate(PROBE);
      const qTheme = /theme=([a-z]+)/.exec(query)[1];
      const pname = (density === 'desktop' ? 'Desktop-' : '') + page_;
      report.themeChecks.push({ page: pname, intended: qTheme, ...(await page.evaluate(THEME)) });
      for (const row of rows) report.rows.push({ ...row, density, theme: qTheme, mode: 'fixture', page: pname });
      report.surfaces.push({ density, theme: qTheme, mode: 'fixture', page: pname, ...(await page.evaluate(SURFACE)) });
      report.names.push({ density, theme: qTheme, mode: 'fixture', page: pname, ...(await page.evaluate(NAMES)) });
      // THE FAILURE SURFACES REPORT TOO, and they are the reason the flag travels WITH the reading: ?fail=1 rejects
      // every /api/ call, so this harness legitimately never opens the stream and the panel is SUPPOSED to say
      // "Sessions unavailable". A judge that guessed that from a page name would be reading a label; this reads the
      // fixture's own answer.
      report.sse.push({ density, theme: qTheme, mode: 'fixture', page: pname, ...(await page.evaluate(SSE)) });
    }
  }

  // THE DEVICE'S LAST-COMMAND OUTCOME, PHOTOGRAPHED (round 96). liveness.ts names this hole in its own
  // comment — "'Failed' is not here because no field reports it per session" — and the device reports it now
  // (last_exit_code), so the panel's session row wears a chip for a NON-ZERO code.
  // (No backticks: this comment is inside the emitted template literal — 52nd time, caught by the emit.) Every seed this harness
  // builds reports no code at all, so without a surface the chip exists on the wire and nowhere a sweep can
  // measure it: the same gap rounds 88-92 closed four times, and the fifth state found the same way.
  //
  // FOUR OF THE FIVE STATES ARE ON ONE PAGE, which is what makes this surface worth its renders: the ssh row's
  // last command FAILED (Some(1) -> the triangle), the busy serial row is WORKING, the first row holds a question
  // (WAITING), and the rest are IDLE. The marks probe compares states WITHIN a family on ONE page, so this is the
  // only surface where a collapsed failed/idle or failed/waiting can be caught at all; off is the fifth and
  // lives on the closed surface. The states that render IDENTICALLY are pinned by tests instead of pixels: exit
  // ZERO and absent both draw nothing, and liveness.test.ts is where that difference lives.
  // (No backticks: this comment is inside the emitted template literal.)
  //
  // PANEL DENSITY ONLY: the desktop shell renders its own tab strip and no side list, so the chip has no
  // desktop surface to photograph — measured, not assumed (the press pass reports .side-row as NOT RENDERED
  // in that density).
  // THE APPROVAL GATE'S DISARMED STATE (round 29 of the standing goal). The harness only ever rendered the gate ARMED,
  // so its hollow ring — a GRAPHIC, so 3:1 — had never been measured by anything, and the live-panel probe found it at
  // 2.56 on the light surface. This surface exists so that state is photographed on every run: the extra rows are the
  // ring's cost, and the alternative was a defect the gates cannot see.
  // THE PLUGIN DOT'S ERROR STATE (round 42 of the standing goal). One click, one failing reply: the playwright card's
  // Start POST answers 500 with the device's own words, so the row's dot takes the error silhouette AND the message is
  // printed beneath it. callApi only throws on an HTTP error status, so a fixture answering 200 with ok:false would
  // have rendered nothing at all. Round 26 recorded that ongoing (playwright RUNNING) hangs a sweep; this is the
  // opposite end of the same control and cannot: a FAILED start never spawns a browser.
  if (wants("pages")) {
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize({ width: 1280, height: 860 });
      await page.goto('http://vale.test/panel/?theme=' + theme + '&mode=idle&sessions=3&pwstart=fail&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(1800);
      await page.evaluate(() => {
        // THE REAL MARKUP, NOT A GUESSED SELECTOR (round 68). The classes come from PluginsPage.tsx:
        // .plug-actions holds the controls and each is a .plug-btn. The first version guessed
        // [class*="plugin"] and matched nothing, so the surface clicked no button, no request failed, and
        // plug-dot[error] had no surface while the fixture next to it was pinned and correct.
        const card = [...document.querySelectorAll('.plug-card, .plugin-card, li')]
          .find((c) => /playwright/i.test(c.textContent || '') && c.querySelector('.plug-btn'));
        const btn = card && [...card.querySelectorAll('.plug-btn')].find((b) => /^Start/.test((b.textContent || '').trim()));
        if (btn) btn.click();
      });
      await page.waitForTimeout(1200);
      const fname = 'PluginStartFail-' + theme;
      const frows = await page.evaluate(PROBE);
      for (const row of frows) report.rows.push({ ...row, density: 'panel', theme, mode: 'plugin-fail', page: fname });
      report.surfaces.push({ density: 'panel', theme, mode: 'plugin-fail', page: fname, ...(await page.evaluate(SURFACE)) });
      report.names.push({ density: 'panel', theme, mode: 'plugin-fail', page: fname, ...(await page.evaluate(NAMES)) });
      report.sse.push({ density: 'panel', theme, mode: 'plugin-fail', page: fname, ...(await page.evaluate(SSE)) });
    }
  }

  // THE MONITOR ALERT STRIP'S TWO TONES (round 39 of the standing goal). The device pushes a monitor-change frame
  // when a watched host changes state, and the strip that renders it is the ONE thing in this panel the device is
  // allowed to interrupt with — so both of its marks (the recovery, .monitor-mark.is-up, and the outage, the base
  // .monitor-mark) exist only behind that frame, and no surface had ever delivered one.
  if (wants("pages")) {
    for (const theme of ['light', 'dark']) {
      for (const [dir, label] of [['up', 'MonitorUp'], ['down', 'MonitorDown']]) {
        await page.setViewportSize({ width: 1280, height: 860 });
        await page.goto('http://vale.test/panel/?theme=' + theme + '&mode=idle&sessions=3&monitorchange=' + dir + '&cb=' + stamp, { waitUntil: 'load' });
        await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
        await page.reload({ waitUntil: 'load' });
        await page.waitForTimeout(1800);
        const mname = label + '-' + theme;
        const mrows = await page.evaluate(PROBE);
        for (const row of mrows) report.rows.push({ ...row, density: 'panel', theme, mode: 'monitor-' + dir, page: mname });
        report.surfaces.push({ density: 'panel', theme, mode: 'monitor-' + dir, page: mname, ...(await page.evaluate(SURFACE)) });
        report.names.push({ density: 'panel', theme, mode: 'monitor-' + dir, page: mname, ...(await page.evaluate(NAMES)) });
        report.sse.push({ density: 'panel', theme, mode: 'monitor-' + dir, page: mname, ...(await page.evaluate(SSE)) });
      }
    }
  }

  // THE BOOT CHIP'S OTHER TONE (round 34 of the standing goal), for the same reason as the approval gate's off state
  // one round earlier: a mark with two tones where only one is ever painted has one unmeasured silhouette, and the
  // note has been naming boot-mark info for rounds.
  if (wants("pages")) {
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize({ width: 1280, height: 860 });
      await page.goto('http://vale.test/panel/?theme=' + theme + '&mode=idle&sessions=3&boot=replaced&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(1800);
      const bname = 'BootReplaced-' + theme;
      const brows = await page.evaluate(PROBE);
      for (const row of brows) report.rows.push({ ...row, density: 'panel', theme, mode: 'boot-replaced', page: bname });
      report.surfaces.push({ density: 'panel', theme, mode: 'boot-replaced', page: bname, ...(await page.evaluate(SURFACE)) });
      report.names.push({ density: 'panel', theme, mode: 'boot-replaced', page: bname, ...(await page.evaluate(NAMES)) });
      report.sse.push({ density: 'panel', theme, mode: 'boot-replaced', page: bname, ...(await page.evaluate(SSE)) });
    }
  }

  if (wants("pages")) {
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize({ width: 1280, height: 860 });
      await page.goto('http://vale.test/panel/?theme=' + theme + '&mode=idle&sessions=3&appr=off&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(1800);
      const aname = 'ApprovalOff-' + theme;
      const arows = await page.evaluate(PROBE);
      for (const row of arows) report.rows.push({ ...row, density: 'panel', theme, mode: 'approval-off', page: aname });
      report.surfaces.push({ density: 'panel', theme, mode: 'approval-off', page: aname, ...(await page.evaluate(SURFACE)) });
      report.names.push({ density: 'panel', theme, mode: 'approval-off', page: aname, ...(await page.evaluate(NAMES)) });
      report.sse.push({ density: 'panel', theme, mode: 'approval-off', page: aname, ...(await page.evaluate(SSE)) });
    }
  }

  if (wants("pages")) {
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize({ width: 1280, height: 860 });
      await page.goto('http://vale.test/panel/?theme=' + theme + '&mode=pending&sessions=6&exitfail=1&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(1800);
      const pname = 'LastFail-' + theme;
      const rows = await page.evaluate(PROBE);
      for (const row of rows) report.rows.push({ ...row, density: 'panel', theme, mode: 'exit-fail', page: pname });
      report.surfaces.push({ density: 'panel', theme, mode: 'exit-fail', page: pname, ...(await page.evaluate(SURFACE)) });
      report.names.push({ density: 'panel', theme, mode: 'exit-fail', page: pname, ...(await page.evaluate(NAMES)) });
      report.sse.push({ density: 'panel', theme, mode: 'exit-fail', page: pname, ...(await page.evaluate(SSE)) });
    }
  }

  // THE SAME FAILURE ON THE ACTIVE SESSION (round 98). The surface above puts it on a quiet row, and the mark is
  // ALSO drawn on the accent-filled ACTIVE TAB — where the state's ink drew 2.16:1 in dark while no surface rendered
  // the combination. One render per theme, mode=idle so nothing outranks the failure, and the ACTIVE tab and row are
  // the ones carrying it.
  if (wants("pages")) {
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize({ width: 1280, height: 860 });
      await page.goto('http://vale.test/panel/?theme=' + theme + '&mode=idle&sessions=3&exitfail=active&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(1800);
      const pname = 'LastFailActive-' + theme;
      const rows = await page.evaluate(PROBE);
      for (const row of rows) report.rows.push({ ...row, density: 'panel', theme, mode: 'exit-fail-active', page: pname });
      report.surfaces.push({ density: 'panel', theme, mode: 'exit-fail-active', page: pname, ...(await page.evaluate(SURFACE)) });
      report.names.push({ density: 'panel', theme, mode: 'exit-fail-active', page: pname, ...(await page.evaluate(NAMES)) });
      report.sse.push({ density: 'panel', theme, mode: 'exit-fail-active', page: pname, ...(await page.evaluate(SSE)) });
    }
  }

  // THE OTHER VERDICT TONE ON THE DEVICE-LOGS CARD (round 100). The card derives its sentence from the four-way
  // table over vale-update.log, so ONE payload can only ever render one tone: the default fixture has a receipt and
  // a start (OK), and this one has the receipt with no start (WARN — the CLI reached the device and the swap never
  // launched). The card's OK tone measured 3.33:1 as text the first time it was rendered at all; a tone with no
  // surface is a tone no sweep can measure, which is the rule rounds 96-99 keep relearning.
  if (wants("pages")) {
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize({ width: 1280, height: 860 });
      await page.goto('http://vale.test/panel/?theme=' + theme + '&mode=idle&sessions=3&logs=warn&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(1500);
      const pname = 'LogsWarn-' + theme;
      await page.evaluate(() => {
        const b = [...document.querySelectorAll('#icon-rail button, .desktop-rail button')].find((x) => /settings/i.test((x.getAttribute('aria-label') || '') + x.textContent));
        if (b) b.click();
      });
      await page.waitForTimeout(1500);
      const rows = await page.evaluate(PROBE);
      for (const row of rows) report.rows.push({ ...row, density: 'panel', theme, mode: 'logs-warn', page: pname });
      report.surfaces.push({ density: 'panel', theme, mode: 'logs-warn', page: pname, ...(await page.evaluate(SURFACE)) });
      report.names.push({ density: 'panel', theme, mode: 'logs-warn', page: pname, ...(await page.evaluate(NAMES)) });
    }
  }

  // THE TWO RECORD VIEWS, WHICH NO SWEEP HAS EVER RENDERED (round 101). The per-session view switch has three
  // tabs — Terminal, Trajectory (the raw audit timeline) and Path (the same work as steps, with a summary) — and
  // every surface in this suite leaves it on Terminal. The harness DOES serve the events when asked
  // (/api/sessions/<id> carries a goal, an approval armed/approved/granted, two commands with exit 0 and exit 1,
  // and their output), so both views have real content to draw; nothing ever clicked the tab. Every style they
  // use — the event dots and their states, the exit badges, the governance chips, the plan rows, the attention
  // rows — has therefore been measured by nothing at all, which is the same hole the History page's empty archive
  // sits in. ONE CLICK, and the cost of not making it was the panel's most information-dense two views.
  if (wants("pages")) {
    for (const [density, path_, vp] of [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]]) {
      for (const theme of ['light', 'dark']) {
        for (const tab of ['Trajectory', 'Path']) {
          await page.setViewportSize(vp);
          await page.goto('http://vale.test' + path_ + '?theme=' + theme + '&mode=idle&sessions=3&cb=' + stamp, { waitUntil: 'load' });
          await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
          await page.reload({ waitUntil: 'load' });
          await page.waitForTimeout(1500);
          await page.evaluate((want) => {
            const btn = [...document.querySelectorAll('.view-switch button, .desktop-view-switch button')]
              .find((b) => (b.textContent || '').trim() === want);
            if (btn) btn.click();
          }, tab);
          // AND OPEN THE ROUNDS, WHICH IS THE CLICK THAT WAS STILL MISSING (round 33). Only the NEWEST round is
          // open by default, and the newest round is the fixture's live reboot — a command with no output yet — so
          // its body is (no output yet) and every event row (and its .traj-ev-dot) lived inside a COLLAPSED
          // round: the family measured 0 of 6 states across 128 surfaces while the view rendered perfectly. The note
          // only started saying so after round 31 added that trailing command, which is a fair illustration of how a
          // fixture change can hide a family: the sweep opened the tab (round 101) but never a round.
          if (tab === 'Trajectory') {
            await page.evaluate(() => {
              for (const head of [...document.querySelectorAll('.traj-round-head')].slice(0, 12)) head.click();
            });
            await page.waitForTimeout(900);
          }
          await page.waitForTimeout(1800);
          const pname = (density === 'desktop' ? 'Desktop-' : '') + tab + '-' + theme;
          const rows = await page.evaluate(PROBE);
          for (const row of rows) report.rows.push({ ...row, density, theme, mode: 'record', page: pname });
          report.surfaces.push({ density, theme, mode: 'record', page: pname, ...(await page.evaluate(SURFACE)) });
          report.names.push({ density, theme, mode: 'record', page: pname, ...(await page.evaluate(NAMES)) });
          report.sse.push({ density, theme, mode: 'record', page: pname, ...(await page.evaluate(SSE)) });
        }
      }
    }
  }

  // THREE STATES OF THE RECORD PAGE THAT NO SURFACE HAS EVER PHOTOGRAPHED (round 102). The History page's first
  // scope is the ARCHIVE, and every sweep has measured it EMPTY: the harness has served a populated one behind
  // ?rows=N since round 69, and no surface ever passed it — so the row list, its identity/reason/when columns and
  // its cost at scale were measured by nothing. Inside a row is the TRAIL of a recorded session, which is the one
  // place an operator can read a session that is over. And the page's SECOND scope ("Runs") reads /api/operation,
  // which no stub answered at all until this round.
  if (wants("pages")) {
    const gotoHistory = async () => {
      await page.evaluate(() => {
        const b = [...document.querySelectorAll('#icon-rail button, .desktop-rail button')].find((x) => /history/i.test((x.getAttribute('aria-label') || '') + x.textContent));
        if (b) b.click();
      });
      await page.waitForTimeout(1500);
    };
    for (const theme of ['light', 'dark']) {
      // (a) THE ARCHIVE WITH CONTENT
      await page.setViewportSize({ width: 1280, height: 860 });
      await page.goto('http://vale.test/panel/?theme=' + theme + '&mode=idle&sessions=3&rows=50&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(1500);
      await gotoHistory();
      const rowsName = 'ArchiveRows-' + theme;
      const rowsA = await page.evaluate(PROBE);
      for (const row of rowsA) report.rows.push({ ...row, density: 'panel', theme, mode: 'archive', page: rowsName });
      report.surfaces.push({ density: 'panel', theme, mode: 'archive', page: rowsName, ...(await page.evaluate(SURFACE)) });
      report.names.push({ density: 'panel', theme, mode: 'archive', page: rowsName, ...(await page.evaluate(NAMES)) });
      report.sse.push({ density: 'panel', theme, mode: 'archive', page: rowsName, ...(await page.evaluate(SSE)) });

      // (b) THE TRAIL INSIDE AN ARCHIVED SESSION — one click, the same page
      await page.evaluate(() => {
        const b = document.querySelector('.archive-row');
        if (b) b.click();
      });
      await page.waitForTimeout(1800);
      const trailName = 'ArchiveTrail-' + theme;
      const rowsB = await page.evaluate(PROBE);
      for (const row of rowsB) report.rows.push({ ...row, density: 'panel', theme, mode: 'archive-trail', page: trailName });
      report.surfaces.push({ density: 'panel', theme, mode: 'archive-trail', page: trailName, ...(await page.evaluate(SURFACE)) });
      report.names.push({ density: 'panel', theme, mode: 'archive-trail', page: trailName, ...(await page.evaluate(NAMES)) });
      report.sse.push({ density: 'panel', theme, mode: 'archive-trail', page: trailName, ...(await page.evaluate(SSE)) });
    }
    for (const theme of ['light', 'dark']) {
      // (c) THE RUNS SCOPE
      await page.setViewportSize({ width: 1280, height: 860 });
      await page.goto('http://vale.test/panel/?theme=' + theme + '&mode=idle&sessions=3&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(1500);
      await gotoHistory();
      await page.evaluate(() => {
        const b = [...document.querySelectorAll('.view-switch button, .desktop-view-switch button')].find((x) => (x.textContent || '').trim() === 'Runs');
        if (b) b.click();
      });
      await page.waitForTimeout(1800);
      const runsName = 'HistoryRuns-' + theme;
      const rowsC = await page.evaluate(PROBE);
      for (const row of rowsC) report.rows.push({ ...row, density: 'panel', theme, mode: 'runs', page: runsName });
      report.surfaces.push({ density: 'panel', theme, mode: 'runs', page: runsName, ...(await page.evaluate(SURFACE)) });
      report.names.push({ density: 'panel', theme, mode: 'runs', page: runsName, ...(await page.evaluate(NAMES)) });
      report.sse.push({ density: 'panel', theme, mode: 'runs', page: runsName, ...(await page.evaluate(SSE)) });
    }
  }

  // THE ACKNOWLEDGEMENT'S LATENCY, MEASURED AGAINST A SLOW NETWORK (round 19). Every response is delayed by
  // slowms, so a control whose feedback waits for the reply cannot hide: the panel's promise is that the pressed
  // control shows its busy state ON THE EVENT. Two controls per density, both themes — the monitor row's check now
  // (a network call with a visible result) and the add form's watch (a POST that also changes the page).
  const ACK_BUDGET_MS = 100;
  if (wants("ack")) {
    for (const [density, path_, vp] of [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]]) {
      for (const theme of ['light', 'dark']) {
        await page.setViewportSize(vp);
        await page.goto('http://vale.test' + path_ + '?theme=' + theme + '&mode=idle&sessions=3&slowms=900&cb=' + stamp, { waitUntil: 'load' });
        await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
        await page.reload({ waitUntil: 'load' });
        await page.waitForTimeout(2200);
        await page.evaluate(() => {
          const b = [...document.querySelectorAll('#icon-rail button, .desktop-rail button')].find((x) => (x.getAttribute('aria-label') || '').toLowerCase() === 'settings');
          if (b) b.click();
        });
        await page.waitForTimeout(2200);
        const name = (density === 'desktop' ? 'Desktop-' : '') + 'Settings-ack-' + theme;
        // THE CURATED PAIR STAYS ON THIS PAGE, AND THE REASON IS MEASURED (round 20). Discovery was tried here
        // first: the Settings page renders the connect form's controls ahead of everything else, so a cap of eight
        // spent itself on three tabs and three unnamed buttons — and the tabs are a FALSE ACCUSATION, because the
        // first one is ALREADY ACTIVE and clicking it has nothing to do. The pass's __calls delta cannot excuse
        // them either: this page polls (update, monitors, vitals, restarts, logs), so a background request lands in
        // almost any window and "this control asked the device" becomes unattributable. The two controls below are
        // the ones whose work is known; discovery belongs on a page where every button does something, which is the
        // Memory page below.
        const rows = await ackPass(page, ['.monitor-btn', '.monitor-add .btn'], ACK_BUDGET_MS, { density, theme, mode: 'ack', page: name });
        report.ack = report.ack || [];
        for (const r of rows) report.ack.push(r);
      }
    }
  }

  // AND THE MEMORY PAGE, whose buttons write and delete device-local records (round 20). One page is not a survey:
  // the Settings surface answers for Settings, and the control nobody named is as likely to live here.
  if (wants("ack")) {
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize({ width: 1280, height: 860 });
      await page.goto('http://vale.test/panel/?theme=' + theme + '&mode=idle&sessions=3&slowms=900&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(2200);
      await page.evaluate(() => {
        const b = [...document.querySelectorAll('#icon-rail button, .desktop-rail button')].find((x) => (x.getAttribute('aria-label') || '').toLowerCase() === 'memory');
        if (b) b.click();
      });
      await page.waitForTimeout(2200);
      const name = 'Memory-ack-' + theme;
      const rows = await ackPass(page, [], ACK_BUDGET_MS, {
        density: 'panel', theme, mode: 'ack', page: name, discover: 4,
        skip: ['.rail-btn', '.desktop-rail-btn', '.tab', '.dtab', '.side-row', '.side-add'],
      });
      report.ack = report.ack || [];
      for (const r of rows) report.ack.push(r);
    }
  }

  // THE UNSET GOAL, WHICH IS THE COMMON CASE (round 90). GoalBar's own comment calls an unset goal "normal (most
  // sessions)" and describes what it renders instead: "a QUIET affordance". Every fixture this harness has ever built
  // gave EVERY session a goal, so the affordance — a dashed-bordered button whose only content is a bare text node,
  // with no .goal-text span inside it — has never been rendered, while the state it replaces has been measured on
  // every page. Third round running that a surface for an unrendered state found the state was not what the sheet
  // alone could prove.
  if (wants("pages")) {
    for (const [density, path_, vp] of [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]])
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize(vp);
      await page.goto('http://vale.test' + path_ + '?theme=' + theme + '&mode=idle&sessions=4&goal=none&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(1800);
      const pname = (density === 'desktop' ? 'Desktop-' : '') + 'NoGoal-' + theme;
      const rows = await page.evaluate(PROBE);
      for (const row of rows) report.rows.push({ ...row, density, theme, mode: 'no-goal', page: pname });
      report.surfaces.push({ density, theme, mode: 'no-goal', page: pname, ...(await page.evaluate(SURFACE)) });
      report.names.push({ density, theme, mode: 'no-goal', page: pname, ...(await page.evaluate(NAMES)) });
      report.sse.push({ density, theme, mode: 'no-goal', page: pname, ...(await page.evaluate(SSE)) });
    }
  }

  // THE HELD SESSION, PHOTOGRAPHED FOR THE FIRST TIME (round 89). held_by_human is the fact the panel is most
  // careful about — SessionControl reads it from the session record and will not flip the button until the server
  // agrees — and every fixture this harness has ever built set it FALSE. So the HUMAN state of the .sc-dot mark
  // (a solid fill against the ai state's inset ring) and the button's .held variant have never been rendered.
  // ONE session is held and the rest are not, because the marks probe compares states within a family and can only
  // see a collision between two states that are both on screen.
  if (wants("pages")) {
    for (const [density, path_, vp] of [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]])
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize(vp);
      await page.goto('http://vale.test' + path_ + '?theme=' + theme + '&mode=idle&sessions=4&held=1&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(1800);
      const pname = (density === 'desktop' ? 'Desktop-' : '') + 'Held-' + theme;
      const rows = await page.evaluate(PROBE);
      for (const row of rows) report.rows.push({ ...row, density, theme, mode: 'held', page: pname });
      report.surfaces.push({ density, theme, mode: 'held', page: pname, ...(await page.evaluate(SURFACE)) });
      report.names.push({ density, theme, mode: 'held', page: pname, ...(await page.evaluate(NAMES)) });
      report.sse.push({ density, theme, mode: 'held', page: pname, ...(await page.evaluate(SSE)) });
    }
  }

  // THE FOURTH SILHOUETTE, PHOTOGRAPHED AT LAST (round 88). The mark language has four states — off, waiting,
  // working, idle — and this file's own note has said for many rounds that the page sweep, which photographs pages
  // and never presses, has never photographed 'off': a closed session is CLIENT state, so no URL parameter can
  // produce it. It is reachable by PRESSING, and the recipe is the harness's: click the tab's x to arm the two-step
  // close, click the confirm's Close, and read AFTER the tab's 0.15s background transition settles. Round 245 made
  // it stable — terminal_close removes the session from every later list answer, so the tombstone no longer lives
  // only inside a timing window.
  //
  // AND IT IS THE ONLY SURFACE WHERE ALL FOUR STATES CAN BE COMPARED AT ONCE, which is the point. The marks probe
  // groups a family's states per page and fails when two of them paint identically; until this surface existed, a
  // page could show working beside idle and never show off beside either. mode=pending keeps the pending approval,
  // so the diamond is here too.
  if (wants("pages")) {
    for (const [density, path_, vp] of [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]])
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize(vp);
      await page.goto('http://vale.test' + path_ + '?theme=' + theme + '&mode=pending&sessions=4&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(1800);
      // 1. arm the two-step close on the LAST tab, so the states the other surfaces rely on stay on screen.
      await page.evaluate(() => {
        const closes = [...document.querySelectorAll('.tab .tab-close')];
        if (closes.length) closes[closes.length - 1].click();
      });
      await page.waitForTimeout(250);
      // 2. confirm it. /api/tools/terminal_close is stubbed and succeeds.
      await page.evaluate(() => {
        const confirm = document.querySelector('.tab-confirm .btn-danger');
        if (confirm) confirm.click();
      });
      await page.waitForTimeout(700);
      const pname = (density === 'desktop' ? 'Desktop-' : '') + 'Closed-' + theme;
      const rows = await page.evaluate(PROBE);
      for (const row of rows) report.rows.push({ ...row, density, theme, mode: 'closed', page: pname });
      report.surfaces.push({ density, theme, mode: 'closed', page: pname, ...(await page.evaluate(SURFACE)) });
      report.names.push({ density, theme, mode: 'closed', page: pname, ...(await page.evaluate(NAMES)) });
      report.sse.push({ density, theme, mode: 'closed', page: pname, ...(await page.evaluate(SSE)) });
    }
  }

  // WHY THE EVIDENCE DRAWER IS STILL NOT MEASURED, recorded so it is not re-attempted from scratch
  // (round 193). THIS HARNESS RENDERS THE REAL BUNDLE — panel.js and panel.css — and drives it with stubbed
  // API responses; it contains no hand-written markup at all. The drawer is opened only by
  // EmbeddedBrowserPane, which talks to the Electron shell's control server on 127.0.0.1:9444, so in a
  // browser there is no pane to open it from. Adding a hand-written drawer block would put the ONLY
  // fabricated markup in a fixture built on the real app, and it would measure my markup rather than the
  // component. The two honest routes are an app-level seam (a URL parameter that opens the drawer, which is
  // a test hook in shipped code) or measuring it inside Electron (which this repository has no runner for).
  // Neither is taken; the gap is real and named rather than assumed.
  // TARGET SIZE, WCAG 2.5.8, the FULL criterion. Nothing measured it before round 162 — the number 24 was
  // already in this suite as the threshold for whether a non-text element is a MARK, which is a different
  // question. Both densities, one render each, recorded like any other surface.
  for (const [density, path_, vp] of wants("pages") ? [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]] : []) {
    await page.setViewportSize(vp);
    await page.goto('http://vale.test' + path_ + '?theme=light&mode=relaxed&sessions=3&cb=' + stamp, { waitUntil: 'load' });
    await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
    await page.reload({ waitUntil: 'load' });
    await page.waitForTimeout(2000);
    report.targets.push({ density, mode: 'rest', ...(await page.evaluate(TARGETS)) });
  }

  // UNSTYLED CLASSES — the mirror of dead CSS, and the failure a PRUNE causes. Same collector the
  // console uses, from the shared core, embedded with JSON.stringify (round 88 shipped one embedded
  // in a template literal and the device received /s+/ where the source said /\s+/: the report listed
  // "btn btn-" and "rail-clu", finding 38 styled classes where the browser sees 221).
  for (const [density, path_, vp] of wants("unstyled") ? [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]] : []) {
    await page.setViewportSize(vp);
    await page.goto('http://vale.test' + path_ + '?theme=light&mode=relaxed&sessions=3&cb=' + stamp, { waitUntil: 'load' });
    await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
    await page.reload({ waitUntil: 'load' });
    await page.waitForTimeout(1500);
    report.unstyled.push({ page: density, ...(await page.evaluate(UNSTYLED)) });
  }

  // HOVER, measured rather than assumed. Nothing had ever looked at it: the static pair sweep reads
  // base rules and every rendered pass measures the resting DOM, while the panel carries 73 :hover
  // rules. Each interactive element is hovered in turn and the page measured while it is hovered.
  for (const [density, path_, vp] of wants("hover") ? [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]] : []) {
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize(vp);
      await page.goto('http://vale.test' + path_ + '?theme=' + theme + '&mode=relaxed&sessions=3&cb=' + stamp, { waitUntil: 'load' });
      await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
      await page.reload({ waitUntil: 'load' });
      await page.waitForTimeout(1500);
      const underAA = [];
      // ONE PER FAMILY, not every instance. Re-running the whole-DOM probe after each hover costs a
      // pass over ~400 nodes, and 31 elements x 4 combinations made the sweep exceed the caller's
      // timeout twice. Hover styles are per-class, so the first element of each distinct class is the
      // same measurement at a quarter of the cost.
      const all = await page.$$('#root button, #root [role="button"], #root a');
      const seenClass = new Set();
      const handles = [];
      for (const h of all) {
        const key = await h.evaluate((el) => (typeof el.className === 'string' ? el.className : el.tagName));
        if (seenClass.has(key)) continue;
        seenClass.add(key);
        handles.push(h);
      }
      for (const h of handles) {
        // SKIP WHAT CANNOT BE HOVERED BEFORE ASKING. hover() waits out its timeout on a hidden or
        // zero-size element, and 31 elements x 4 passes of that made this pass longer than the whole
        // rest of the sweep (the first live run timed out at the call boundary, not in the page).
        // (No backticks in this comment: it lives inside the emitted template — seventh time.)
        const box = await h.boundingBox();
        if (!box || box.width < 2 || box.height < 2) continue;
        try {
          await h.hover({ timeout: 400 });
        } catch (e) {
          continue;   // covered by something else in this viewport
        }
        await page.waitForTimeout(90);
        for (const r of await page.evaluate(PROBE)) {
          const need = r.need ?? 4.5;
          if (r.cr !== null && !r.inactive && r.cr < need) {
            underAA.push(r.sel + ' "' + String(r.text).slice(0, 16) + '" ' + r.cr + '<' + need + ' painted ' + r.paint + ' on ' + r.surface + ', ' + r.size + 'px ' + r.kind);
          }
        }
        await page.mouse.move(2, 2);
      }
      report.hover.push({ density, theme, interactive: handles.length, underAA: [...new Set(underAA)] });
    }
  }

  // REDUCED MOTION, measured rather than assumed: render both densities with the preference
  // EMULATED and ask the page which elements still have a running transition or animation. Reading
  // the stylesheet cannot answer this — a media query adds no specificity, so the answer depends on
  // cascade order, selector scope and xterm's runtime-injected sheet.
  for (const [density, path_] of wants("motion") ? [['panel', '/panel/'], ['desktop', '/desktop/']] : []) {
    await page.setViewportSize(density === 'panel' ? { width: 1280, height: 860 } : { width: 1440, height: 900 });
    // MEASURE IT TWICE, because "nothing animates under reduce" is only evidence if SOMETHING animates
    // without it. The old report carried one number, so a page with no transitions at all and a page
    // whose transitions were correctly suppressed both read as "animating: []" — the same vacuity this
    // suite keeps finding in its own checks. Normal first, then the same page with the preference set.
    await page.emulateMedia({ reducedMotion: null });
    await page.goto('http://vale.test' + path_ + '?theme=light&mode=idle&sessions=3&cb=' + stamp, { waitUntil: 'load' });
    await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
    await page.reload({ waitUntil: 'load' });
    await page.waitForTimeout(1600);
    const normal = await page.evaluate(MOTION);
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.reload({ waitUntil: 'load' });
    await page.waitForTimeout(1200);
    const reduced = await page.evaluate(MOTION);
    report.motion.push({
      density,
      normal: normal.animating.length,
      reduced: reduced.animating.length,
      stillAnimating: reduced.animating.slice(0, 6),
    });
  }
  await page.emulateMedia({ reducedMotion: null });

  // Reflow at the two widths WCAG 1.4.10 names, panel density only: the desktop density needs the
  // width it has, and its tab strip is the standard's own toolbar exception.
  for (const width of wants("reflow") ? [640, 320] : []) {
    await page.setViewportSize({ width, height: 800 });
    await page.goto('http://vale.test/panel/?theme=light&mode=idle&sessions=3&cb=' + stamp, { waitUntil: 'load' });
    await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
    await page.reload({ waitUntil: 'load' });
    await page.waitForTimeout(1500);
    report.reflow.push({ width, ...(await page.evaluate(REFLOW)) });
  }
  await diag("done rows=" + (report.rows || []).length + " findings-source-ready pid=" + process.pid);
  fs.writeFileSync(REPORT_PATH, JSON.stringify(report));
  console.log(JSON.stringify({ rows: report.rows.length, surfaces: report.surfaces.length, names: report.names.length }));
  await close();
})().catch((e) => { console.error('FATAL', e.message); process.exit(1); });`;
  // THE EMITTED SCRIPT MUST PARSE — which is what the 23 backtick incidents actually cost, and the only
  // check that works HERE. A blanket backtick search is wrong for this emitter: the script it builds
  // legitimately CONTAINS backticks, because it defines nested template sources of its own (the probe, the
  // motion probe, the timing probe). My first attempt searched for one, found the script's own, and refused
  // a perfectly good emission — the check told me more about itself than about the script.
  // Compiling asks the question that matters: a backtick in a comment ends the outer literal early and the
  // emitted 37 KB script stops parsing, wherever the damage happens to land.
  try {
    new Function(script);
  } catch (e) {
    throw new Error(`the emitted script does not parse: ${e.message}`);
  }
  return script;
}

function judge(file) {
  const report = JSON.parse(readFileSync(file, "utf8"));
  // WHAT THIS REPORT WAS SUPPOSED TO COVER. The sweep outgrew its caller's timeout, so it can be run
  // in passes — and a partial run that reported "nothing found" would read exactly like a clean full
  // one. The caller states its expectation; the judge fails when the report is missing any of it.
  const expect = (process.argv.find((a) => a.startsWith("--expect=")) || "--expect=all").slice("--expect=".length);
  const missing = expect === "all"
    ? (report.passes === "all" ? [] : [`the run measured only "${report.passes}"`])
    : expect.split(",").map((x) => x.trim()).filter((x) => !(report.passes === "all" || (report.passes || "").split(",").map((y) => y.trim()).includes(x)));
  const coverage = missing.map((m) => `INCOMPLETE REPORT — ${m}; the absences below prove nothing`);
  const findings = [...coverage, ...judgeReport(report, {
    // CLASSES WITH NO MATCHING RULE THAT ARE NOT DEFECTS, each with the mechanism named. Measured
    // round 90: the panel density renders 1123 styled classes and eleven such names, and ten of the
    // eleven are xterm.js's own DOM — styled by a stylesheet it INJECTS AT RUNTIME, which a CSSOM
    // walk cannot see (the same runtime injection that defeated the reduced-motion override in round
    // 77). The last two are ours and also deliberate: `terminal` is a VIEW NAME on an element already
    // carrying `.view`, and `serial` rides a [data-kind] attribute rule.
    implicitStates: {
      "xterm-viewport": "xterm.js DOM, styled by the sheet it injects at runtime",
      "xterm-screen": "xterm.js DOM, styled by the sheet it injects at runtime",
      "xterm-helpers": "xterm.js DOM, styled by the sheet it injects at runtime",
      "xterm-helper-textarea": "xterm.js DOM, styled by the sheet it injects at runtime",
      "xterm-scroll-area": "xterm.js DOM, styled by the sheet it injects at runtime",
      "xterm-char-measure-element": "xterm.js DOM, styled by the sheet it injects at runtime",
      "xterm-width-cache-measure-container": "xterm.js DOM, styled by the sheet it injects at runtime",
      "xterm-decoration-container": "xterm.js DOM, styled by the sheet it injects at runtime",
      "composition-view": "xterm.js DOM (IME composition), styled by the sheet it injects at runtime",
      terminal: "a VIEW NAME; the element's .view class does the styling",
      // PRUNED: `serial`, whose reason was "a session-kind modifier; the element is painted by its
      // [data-kind] rule" (round 24). The class it exempts no longer exists: the mark language's TabBar emits
      // `className="mark tab-dot" data-kind={s.kind}` and the sheet paints the kind with
      // `.tab-dot[data-kind="serial"]`, so nothing puts a bare `serial` on screen. MEASURED, not assumed: the
      // unstyled pass (1129 styled classes per density over 2 pages) reports 11 unstyled names — the eight
      // xterm ones, composition-view, terminal and warn — and `serial` is not among them, while the declared
      // count was 12. The paragraph above counted "eleven such names ... the last two are ours" when this entry
      // was written; the last two are `terminal` and `warn` now, which is what the same run says.
      warn:
        "the boot chip's tone modifier. The BASE rule paints it — .boot-mark is the warn triangle and .boot-mark.info is the exception — so the name needs no rule of its own. Round 121 followed it anyway, and found a real defect behind it: the triangle used --state-warn, which measures 2.80 on the dark chip surface against the 3:1 a graphic needs. Fixed to --warn-ink (6.45 / 8.76).",
    },
    // PAGES WHOSE SECOND LOUD ELEMENT IS NAVIGATION. Measured round 42, with the sweep's OWN theme and loud
    // readings, on every rail page in both densities: light is 0 loud on six of six pages in the panel and 0-1 in
    // the desktop; DARK is the one that reaches two, on the Terminal page only, and the two elements are
    //
    //     panel    button.rail-btn   1444px2   which page you are on
    //              div.tab           3254px2   which session you are looking at
    //     desktop  button.desktop-rail-btn 1600px2 + button.btn-new 1915px2 (the primary ACTION, which
    //              state-colour-check deliberately protects: an accent button is an action, not a state)
    //
    // The loud axis exists to catch a page with two competing FOCAL POINTS — nothing on this page is about the
    // rail or the tab strip, and the terminal canvas behind them is not loud at all. Whether the dark theme should
    // DESATURATE one of these is a design question, and it is recorded as one rather than settled here: the same
    // family of question as inbox row 17 (a halo meaning "lit" on one surface and "in flight" on another).
    //
    // THE NAME IS A PREFIX, so this also covers `panel-Terminal-16-sessions` — measured at 1 loud today, so it is
    // not hiding a known defect, but a change that made THAT page shout would pass because of this entry. Said out
    // loud here because a suppression that quietly widens is the thing this ledger keeps warning about.
    twoloud: ["panel-Terminal", "desktop-Terminal"],
    // THIS HARNESS'S OWN BLIND SPOT, named rather than filtered silently. `#tabs` measures ~0-185px
    // in a plain browser and 211px on the device, so the tab strip's children report as overflowing
    // containers here and nowhere else; and the 320px document scroll is the SAME artifact (round 50
    // traced every offending element to a tab inside `#tabs`). A real defect in the strip would have
    // to be judged on the device, which is why this exemption is narrow and printed on every run.
    ignore: [
      {
        // THE SAME DOT, ON THE OTHER PATH, AND ORDER-INDEPENDENT ON PURPOSE: rows write "2.33 panel/Terminal
        // div.rail-dot" and hover writes "div.rail-dot \"\" 2.33<3", so one ordered pattern matches one path
        // and not the other — which is exactly how this exemption came to cover rows alone (round 214). The
        // value stays in the test, so a DIFFERENT ratio on this element is still a finding on either path.
        test: (text) => /div\.rail-dot/.test(text) && /2\.33/.test(text),
        reason: "the working rail dot's halo is emphasis, not the signal — the fill carries the state and clears 3:1 in both themes (measured, round 212)",
        // IT STAYS, AND THE NUMBER IS WHY (round 22). A run with the hover axis measured 14/14 and 11/11 interactive
        // elements on four surfaces with `underAA: []` — nothing on the hover path is below AA today, so this entry
        // matches nothing while the state it guards keeps passing. The pattern is UNANCHORED (`/div\.rail-dot/`
        // matches the row's sel, which is `div.mark.rail-dot` since the mark language gained data-live), so a hover
        // row of that shape WOULD still be set aside — which is what makes this a guard rather than weight.
        dormant:
          "4 hover surfaces, 14/14 and 11/11 interactive, underAA empty (round 22) — nothing to excuse today; the unanchored pattern still matches the row shape, so it stays for the state it guards",
      },

      // THE ACTIVE TAB'S DOT: THE RING PREFERENCE DOES NOT FIRE, AND THAT IS NOW REPRODUCIBLE (round 206).
      //
      // Round 143 saw the white ring and called these rows an artifact; round 201 added the context field
      // that proved the element IS the active tab's dot; round 205 measured that context on every occurrence.
      // This round measured the thing itself, in the sweep's own conditions (harness, 1280x860, dark, idle,
      // sessions=4), and both halves at once:
      //
      //     the active dot   active=true   box-shadow "rgb(255, 255, 255) 0px 0px 0px 1px"
      //     the probe's row  span.tab-dot   cr=1.16   paint=rgb(217, 72, 15) (background)   <-- the FILL
      //
      // `painterOf` is documented to put the ring first, and its condition is satisfied by that shadow:
      // the colour parses as rgb(255,255,255), and the px tokens are [0, 0, 0, 1], so px[0]===0 && px[1]===0
      // && px[3]>0 all hold. The emitted probe's regex was verified by evaluating the literal the browser
      // evaluates — it is /\s+/, not /\\s+/, so the branch is not defeated by escaping either.
      //
      // SO THE BRANCH IS REACHED AND DOES NOT TAKE, and the next step is to log its INPUTS from inside the
      // emitted probe rather than reason about them from outside: hand it that exact element and that exact
      // box-shadow string and see which condition fails. Everything else about this row is settled.
      {
        // Narrower than a regex on the text: the 320px scroll is this harness's artifact ONLY when
        // every offending scroller is a tab child (round 50 traced them there). A reflow failure
        // from anything else is a real finding and stays one.
        test: (text, entry) =>
          /^reflow @/.test(text) && !!entry && entry.sideScrollers.every((sc) => /tab/.test(sc)),
        reason: "the 320px document scroll comes from tab children inside #tabs — a harness artifact",
      },
    ],
  })];
  // DECORATIVE GRAPHICS: drawn to DELIMIT, not to inform. WCAG 1.4.11 applies to a non-text element that
  // CARRIES MEANING; a chip's 1px hairline does not, and this one measures 1.2 against a surface it was
  // never meant to contrast with. Named here rather than silently dropped — the same rule the rest of
  // this suite follows — and the waived rows are PRINTED on every run so the exemption stays visible.
  const DECORATIVE = [
    // PRUNED: THE WORKING DOT'S HALO WAIVER (round 21 of the standing goal). The entry was `/^div\.rail-dot$/`
    // with the band 2.25-2.45, written when the rows path reported that mark by that class string. The mark
    // language gained `data-live` and the row's selector became `div.mark.rail-dot`, so the pattern has matched
    // NOTHING for several rounds — measured, not assumed: across the whole 126-surface report it matches 0 of
    // 6,876 rows, and the 68 rows that DO name that element are all above their bar (6.50 light / 10.99 dark
    // against 3, and the dot's fill is held at exactly 3.00 by the panel gate, which fails on a mutation to
    // #8a2a07 at 1.90). Nothing needed the exemption any more, which is the definition of weight that stops
    // earning its place. The HOVER path's exemption lives in `ignore` and is untouched; if the halo ever
    // returns as a measured row, the band and its reason are in this file's history and in the design ledger.
    {
      // MEASURED, AND ONE WORD OF THE OLD REASON WAS WRONG (round 203). It read "its meaning is its text
      // (contrast-fixed for this chip already) and its dot" — THERE IS NO DOT. The chip is text plus a revoke
      // button, and the numbers this run produces are: the waived outline at 1.19 (it delimits the pill), the
      // command text at worst 5.53 of 4.5 across 88 rows, and the revoke control at 5.33 of 4.5. The two
      // contrast fixes the CSS documents — --muted at 4.31 for an 11px mono label, and --faint at 2.33 for the
      // one control that can undo a grant — both hold. The outline is the pill's edge; the word is the signal.
      match: /^span\.approval-grant$/,
      // FOUR SURFACES, FOUR RATIOS — 1.19, 1.20, 1.25, 1.27, measured on the device round 95 — because the outline
      // composites over a different surface on each.
      //
      // "AND NOTHING ELSE" IS TRUE NOW (round 23). The band was 1.10-1.35, which is ~0.09 wider on each side than any
      // ratio this suite has ever seen: a drift to 1.12 or 1.33 — real movement toward the 3:1 bar — would have been
      // waived silently. Measured across 32 rows on 126 surfaces: 1.19-1.27, four distinct values. The band is that
      // range plus the declared slack, and the slack is the only margin left to argue about.
      values: [[1.17, 1.29]],
      // PROBE ROUNDING ONLY: the ratios are printed to two decimals, so a true 1.185 reports as 1.19 and a band
      // written at the printed value would refuse it. Two hundredths is the smallest allowance that survives that.
      slack: 0.02,
      reason: "the grant chip's outline delimits the pill at 1.19; the signal is its command text (worst 5.53 of 4.5) and its revoke control (5.33 of 4.5) — both measured every run",
    },
    {
      // THE MENU'S ICON CHIP: ITS BACKGROUND DELIMITS, AND ITS GLYPH IS NOW MEASURED TOO (round 92, corrected
      // round 95). The first photograph of the new-session menu reported span.nm-ico at 1.05 dark / 1.10 light — a
      // 22px chip whose background is a subtle surface behind a coloured glyph, which is what a chip's background is
      // for. What this entry silences is THAT BACKGROUND.
      //
      // THE REASON IT CARRIED FOR FIFTY ROUNDS WAS TRUE WHEN WRITTEN AND IS NOW FALSE, which is why it is worth the
      // line: "the GLYPH ITSELF IS NOT MEASURED — the probe excludes SVG by design". Rounds 93-94 changed exactly
      // that — the svg ROOT is let through, and its paint counts where a shape computes it — so the per-kind lane
      // colour that carries this menu's meaning (--lane-ds for ssh, --lane-or for serial) DOES have a row now, and it
      // clears the 3:1 bar on every surface the sweep renders. A waiver that still claims its signal is unmeasured
      // would stop the next reader looking for the finding that can now appear.
      match: /^span\.nm-ico$/,
      // 1.05 light / 1.10 dark, the chip's own background — and the band is now that range plus the slack rather
      // than 1.00-1.15, which carried 0.05 of margin on each side that no measurement justified (round 23).
      values: [[1.03, 1.12]],
      slack: 0.02,
      reason: "the icon chip's BACKGROUND delimits a coloured glyph at 1.05/1.10; the glyph itself is measured by the SVG rule since round 94 and clears 3:1 — the lane colour it carries has its own row now",
    },
  ];
  const waived = [];
  // TARGET SIZE, WCAG 2.5.8, AS WRITTEN: undersized AND without the spacing that would save it. A check
  // that stopped at the size would flag a dozen compact-but-fine controls and be turned off within a week,
  // which is why the criterion has the second clause and why this uses it.
  // THE REPORT MUST NOT LIE ABOUT WHAT IT RENDERED. Round 175 shipped a two-theme fixture whose rows all
  // said `theme: light` while the URLs said dark — the renders were right and the report was wrong, so a dark
  // regression would have been filed under light. This reads the theme off the PAGE and fails when it
  // disagrees with what was navigated to. An empty stored value is not judged: an app that reads the theme
  // from the URL alone would legitimately have nothing to store.
  for (const t of report.themeChecks || []) {
    const seen = t.stored || t.attr;
    if (seen && seen !== t.intended) {
      findings.push(`theme: ${t.page} was navigated as "${t.intended}" and rendered "${seen}" — the report would be describing a page it did not render`);
    }
  }
  // IMMEDIATE FEEDBACK HAS A BUDGET (round 19). "Pressed and acknowledged states fire on the EVENT, not on the
  // network, inside a stated budget" — so the budget is stated (100ms, well under any round trip and about six
  // frames) and the measurement is the gap between the press and the first visible acknowledgement, taken against a
  // fixture that delays every reply by 900ms. A control that answers only after the reply cannot pass this: the
  // point is not that the device is slow, it is that the interface must not be.
  for (const a of report.ack || []) {
    const where = `${a.density || '?'}${a.page ? ' ' + a.page : ''}`;
    if (a.note) { console.log(`note: ${where} ${a.sel} — ${a.note}`); continue; }
    if (!a.acked) {
      // ONLY WHERE THERE WAS SOMETHING TO WAIT FOR. A control that asked the device nothing (a tab switching a
      // snippet, a disclosure) cannot be late: its row says so rather than becoming a finding.
      if (a.asked === false) {
        console.log(`note: ${where} ${a.sel} — asked the device nothing, so there was nothing to acknowledge`);
        continue;
      }
      // WHAT THIS CAN HONESTLY CLAIM: the control never acknowledged the press in the window. Whether it asked the
      // device is NOT attributable from a request counter on a page that polls for its own reasons, so the finding
      // does not say it did.
      findings.push(`${where}: ${a.sel} (${a.where}) never acknowledged the press — no busy state and no painted change within the window (${a.size})`);
      continue;
    }
    // EVERY ROW'S NUMBERS, ON EVERY RUN (round 26). The judge reported only the failures, so a CI-only failure could
    // not be compared with a clean device run without re-running both by hand: eight controls "never acknowledged"
    // in CI and answered in 6-13ms on the device, same sweep, same fixture. A measurement nobody can read is a
    // measurement nobody can check.
    console.log(
      `note: ack ${where} ${a.sel} — acked=${a.acked} via=${a.via || "none"} ms=${a.msToAck === null ? "-" : a.msToAck} budget=${a.budgetMs} presses=${a.attempts || 1} asked=${a.asked !== false} calls=${a.calls}/${a.callsInWindow}`,
    );
    if (a.acked && (a.attempts || 1) > 1) {
      console.log(`note: ${where} ${a.sel} acknowledged only on the SECOND press — the first sample saw nothing, which on a loaded machine is a timing artifact and on a slow device is a real delay worth watching`);
    }
    if (typeof a.msToAck === "number" && typeof a.budgetMs === "number" && a.msToAck > a.budgetMs) {
      findings.push(`${where}: ${a.sel} (${a.where}) acknowledged the press after ${a.msToAck}ms — the budget is ${a.budgetMs}ms, so this feedback waited on the ${a.msToClear}ms network round trip instead of firing on the event`);
    }
  }
  {
    const acked = (report.ack || []).filter((a) => a.acked);
    if (report.ack && report.ack.length && !acked.length) {
      findings.push(`the acknowledgement pass measured ${report.ack.length} control(s) and NONE acknowledged — a pass that proves nothing is not a pass`);
    }
  }

  for (const t of report.targets || []) {
    // THE LABEL NAMES THE PAGE AND THE STATE, because this axis now measures TWO of them: the resting page and the
    // state a hover reveals (`mode: 'reveal'`). A finding that says only "panel" cannot be reproduced.
    const where = `${t.density || '?'}${t.page ? ' ' + t.page : ''}${t.mode ? ' ' + t.mode : ''}`;
    for (const u of t.distinct || []) {
      if (!u.passesBySpacing) {
        findings.push(`target size (${where}): ${u.sel} is ${u.w}x${u.h} and its nearest neighbour is ${u.nearest}px away — 2.5.8 wants 24x24 or 24px of spacing ("${u.text}")`);
      }
    }
  }
  for (const r of failures(report.rows)) {
    // A WAIVER IS FOR THE RATIO IT WAS MEASURED AT, NOT FOR THE ELEMENT (round 95). This used to be
    // `DECORATIVE.find((d) => d.match.test(String(r.sel)))` and nothing else, so an entry written for one number set
    // aside EVERY ratio that element could ever produce — which is what round 92 caught the rail-dot entry claiming
    // it did not do ("only 2.33 is set aside, so a DIFFERENT ratio on the same element is still a finding": true of
    // the hover path, false of this one). Each entry carries the band it was measured in now, a row outside every
    // band is a finding, and the finding says which band refused it — otherwise the reader sees a bare ratio and
    // cannot tell a new defect from a waiver that moved.
    const why = DECORATIVE.find((d) => d.match.test(String(r.sel)));
    if (why && why.values.some(([lo, hi]) => typeof r.cr === "number" && r.cr >= lo && r.cr <= hi)) {
      waived.push(`${r.sel} ${r.cr} — ${why.reason}`);
      continue;
    }
    if (why) {
      findings.push(
        `${r.sel} ${r.cr} on ${r.surface} — the DECORATIVE entry for this element waives ${why.values.map(([lo, hi]) => `${lo}-${hi}`).join(" / ")}, and this is a DIFFERENT value: a waiver is for the ratio it was measured at, not for the element`,
      );
      continue;
    }
    if (findings.length < 10 + coverage.length) {
      // WHAT THE PROBE MEASURED, NOT JUST THE RATIO (round 73). The row has carried `paint`, `surface`, `kind` and
      // `size` all along and the finding printed none of them, so a number like "2.33" arrived with no way to tell
      // which colour on which surface it was — three rounds went into reproducing an 8px dot because this line did
      // not say what it had looked at. The idle pass learned the same lesson in round 69 (naming the PARENT turned
      // an undiagnosable finding into `span.approval-left x6`); this is the contrast axis taking it.
      // AND THE THEME, which every row carries and this line left out (round 75). `panel/Settings` names a density and
      // a page; the light and dark passes produce the SAME name, so a finding about a dark page and a finding about a
      // light one read identically — and four rounds of this arc went into asking which one CI meant. The surface the
      // row landed on is already printed; the pass it came from is the other half of "say what you measured".
      findings.unshift(`${r.cr} ${r.density}/${r.page} [${r.theme}] ${r.sel} "${String(r.text).slice(0, 24)}" — painted ${r.paint} on ${r.surface}, ${r.size}px ${r.kind}, needs ${r.need}`);
    }
  }
  console.log(reportSummary("panel", report));
  // THE TIMING DATA, SAID OUT LOUD. It has been collected on every page since the timing pass was added and
  // read by nothing — not judged (a wall-clock budget would fail on a loaded CI box, which is why it is not a
  // finding) and not printed either, so a ten-fold regression in boot cost would have been invisible. The
  // worst page is the one worth seeing: a report that prints 40 timings is a report nobody reads.
  {
    const t = (report.timing || []).filter((x) => typeof x.toFirstRowMs === 'number');
    if (t.length) {
      const worst = t.reduce((a, b) => (b.toFirstRowMs > a.toFirstRowMs ? b : a));
      console.log(
        `note: boot timing — worst of ${t.length} surfaces: ${worst.toFirstRowMs}ms to first row ` +
          `(${worst.density}/${worst.mode}, first paint ${worst.firstPaintMs}ms, ${worst.nodes} nodes)`,
      );
    }
  }
  if (coverage.length) console.error(`\n${coverage.join("\n")}`);
  if (unmeasurable(report.rows).length) console.log(`note: ${unmeasurable(report.rows).length} node(s) unmeasurable`);
  // WHICH MARK STATES THE RUN RENDERED, AGAINST WHICH THE SHEET DECLARES (round 26). The collision check compares
  // the silhouettes of the states a surface HAPPENS to render, so a family with six declared states and three
  // rendered has half its shapes unverified — and a collision among the unrendered half cannot be seen at all. This
  // is the round-96 rule ("a state with no surface cannot be measured") applied to the whole mark vocabulary, which
  // is the one place the objective asks for a silhouette PER STATE.
  //
  // A NOTE, with the counts, for the same reason the other three notes carry theirs: a run that measured one axis
  // renders few families, and every state then looks unrendered.
  {
    const seenByFamily = new Map();
    for (const s of report.surfaces || []) {
      for (const entry of (s.marks && s.marks.families) || []) {
        const m = /^([^[]+)\[([^\]]*)\]$/.exec(String(entry));
        if (!m) continue;
        if (!seenByFamily.has(m[1])) seenByFamily.set(m[1], new Set());
        for (const st of m[2].split(",")) if (st) seenByFamily.get(m[1]).add(st);
      }
    }
    // THE FAMILIES COME FROM THE SHEET, NOT FROM THE RUN (round 27). Enumerating what rendered hides the worst case:
    // a family that renders NOWHERE simply does not appear, so the note cannot name it — and the device run that
    // rendered cmd-dot's `running` showed exactly that, because `traj-ev-dot` had disappeared from the list instead
    // of being reported at zero. The sheet's state selectors are the vocabulary; the run is the evidence about it.
    const sheetPath = "agent/resources/panel/panel.css";
    let css = "";
    try {
      css = readFileSync(sheetPath, "utf8");
    } catch (e) {
      css = "";
    }
    if (css) {
      // COMMENTS FIRST, so prose about a selector is not read as one (the lesson `css-vars-check` and
      // `retired-colours-check` both record from their own first runs).
      css = css.replace(/\/\*[\s\S]*?\*\//g, "");
      for (const line of markCoverageNotes(css, report)) console.log(line);
    }
  }

  // HOW WIDE IS A BAND, MEASURED AGAINST WHAT THE RUN SAW (round 23). A DECORATIVE entry waives a RATIO, not an
  // element (round 95), and the band is what decides: a row inside it is set aside, a row outside it is a finding.
  // The failure that leaves no trace is the opposite direction — a band WIDER than its evidence excuses a drift
  // nobody measured, and the grant chip's own reason claimed "the band covers what was measured and nothing else"
  // while a run sees 1.19-1.27 inside a band of 1.10-1.35. Nine hundredths of unearned margin on each side is a
  // quiet hole, so the note names the numbers for every banded entry, with the run's scope, and each entry declares
  // the slack it needs for probe rounding.
  const BAND_SLACK = 0.02;
  {
    const seen = new Map();
    for (const r of report.rows || []) {
      const d = DECORATIVE.find((x) => x.match.test(String(r.sel)));
      if (!d || typeof r.cr !== "number") continue;
      if (!seen.has(d)) seen.set(d, []);
      seen.get(d).push(r.cr);
    }
    for (const d of DECORATIVE) {
      const ratios = seen.get(d) || [];
      if (!ratios.length || !d.values) continue;
      const lo = Math.min(...ratios);
      const hi = Math.max(...ratios);
      const slack = typeof d.slack === "number" ? d.slack : BAND_SLACK;
      // PER SIDE, NOT THE MINIMUM OF THE TWO. The first version took `Math.min(lo - bandLo, bandHi - hi)`, which
      // lets a wide side hide behind a tight one: a run that saw a single 1.19 in a 1.17-1.29 band reported no margin
      // at all, while the upper side carried a tenth nobody had measured — the exact hole this note exists to find,
      // hidden by the arithmetic written to find it.
      //
      // AND AN EPSILON, because the margin is computed in binary floating point: 1.29 - 1.27 is 0.020000000000000018,
      // so an exact band (observed range plus exactly the declared slack) reported itself as 0.02 over 0.02 — a
      // warning about the arithmetic rather than about the band.
      const worstBelow = Math.max(...d.values.map(([a]) => lo - a));
      const worstAbove = Math.max(...d.values.map(([, b]) => b - hi));
      if (worstBelow > slack + 1e-9 || worstAbove > slack + 1e-9) {
        console.log(
          `note: ${d.match} waives ${d.values.map(([a, b]) => `${a}-${b}`).join(" / ")} and this run saw ${lo}-${hi} (${new Set(ratios).size} distinct over ${ratios.length} row(s)) — margin ${worstBelow.toFixed(2)} below and ${worstAbove.toFixed(2)} above against a declared slack of ${slack}; tighten the band or say why the margin is real`,
        );
      }
    }
  }

  // A WAIVER NOBODY USED IS DEAD WEIGHT IN THE ONE LIST A READER CONSULTS (round 21). Every entry in DECORATIVE is
  // permission for an element at a measured ratio; when the element stops rendering under that selector (the mark
  // language changed the class string, and `/^div\.rail-dot$/` matched nothing for several rounds) the entry is a
  // reason nobody is using — and it is exactly the kind of sentence the next reader trusts without checking.
  //
  // A NOTE, NOT A FINDING: this list is judged per run, and a run that measures one axis (the ack pass alone) has
  // rows from nothing else — every entry would look stale. The note says how many rows were looked at, so a reader
  // can tell a real stale entry from a partial run.
  {
    const used = new Set();
    for (const r of report.rows || []) {
      const d = DECORATIVE.find((x) => x.match.test(String(r.sel)));
      if (d) used.add(d);
    }
    const unmatched = DECORATIVE.filter((d) => !used.has(d));
    if (unmatched.length) {
      console.log(
        `note: ${unmatched.length} of ${DECORATIVE.length} DECORATIVE entr(ies) matched NO row in this run (${(report.rows || []).length} rows over ${(report.surfaces || []).length} surface(s)) — a waiver nothing uses is weight; prune it or say why it stays:`,
      );
      for (const d of unmatched) console.log(`  ${String(d.match)}`);
    }
  }
  if (waived.length) {
    console.log(`note: ${waived.length} decorative graphic(s) set aside, each with its reason:`);
    for (const w of [...new Set(waived)]) console.log(`  ${w}`);
  }
  if (!findings.length) {
    console.log("panel design sweep OK: nothing above found a defect");
    return 0;
  }
  console.error(`\n${findings.length} finding(s):\n  ` + findings.join("\n  "));
  return 1;
}

if (mode === "--emit") {
  const out = browserScript();
  // THE EMITTER NAMES WHAT IT BORROWS, and a borrowed helper that CALLS another one needs that one embedded too:
  // missing it, the emitted file still parses (it throws when reached), every local gate passes because they read
  // the artifact's text, and CI finds out. The shared assertion is in lib/design-sweep.mjs.
  assertEmbedded(out, ["focusPass", "pressDelta", "pressPass", "discoverPressTargets", "revealPass", "ackPass", "idlePass", "motionPass"]);
  process.stdout.write(out);
} else if (mode === "--judge") {
  const file = process.argv[3];
  if (!file) {
    console.error("usage: panel-design-sweep.mjs --judge <report.json>");
    process.exit(2);
  }
  process.exit(judge(file));
} else {
  console.error(readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n").slice(1, 20).join("\n"));
  console.error("\nusage: panel-design-sweep.mjs --emit | --judge <report.json>");
  process.exit(2);
}
