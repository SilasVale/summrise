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
import { fileURLToPath } from "node:url";
import { failures, unmeasurable, PROBE_SOURCE } from "./lib/contrast-probe.mjs";
import { pageChecks, judgeReport, reportSummary, UNSTYLED_SOURCE, focusPass, motionPass } from "./lib/design-sweep.mjs";

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
    const key = typeof el.className === 'string' && el.className ? '.' + el.className.trim().split(/\\s+/).join('.') : el.tagName.toLowerCase();
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

function browserScript() {
  return `const fs = require('fs');
const HARNESS = 'C:\\\\ProgramData\\\\Vale\\\\pwout\\\\panel-harness.html';
const PROBE = ${JSON.stringify(PROBE_SOURCE)};
${pageChecks("#root")}
const UNSTYLED = ${JSON.stringify(UNSTYLED_SOURCE)};
const focusPass = ${focusPass.toString()};
const motionPass = ${motionPass.toString()};
${MOTION}
${TIMING}
(async () => {
  const { acquireBrowser } = require(process.env.VALE_BROWSER_HELPER);
  const { page, close } = await acquireBrowser();
  const html = fs.readFileSync(HARNESS, 'utf8');
  const stamp = Date.now();
  await page.route('http://vale.test/**', (route) =>
    route.fulfill({ status: 200, contentType: 'text/html; charset=utf-8', headers: { 'cache-control': 'no-store' }, body: html }));

  const report = {
    // WHICH PASSES RAN, recorded in the report itself. The sweep outgrew its caller's timeout in round
    // 90 (a check that cannot complete is a check that will quietly stop running), so passes are
    // selectable — and a PARTIAL report must not read as a clean one, which is why this list travels
    // with the data and the judge refuses a report that does not say it covered everything.
    passes: ${JSON.stringify(PASSES)},
    rows: [], surfaces: [], reflow: [], names: [], timing: [], focus: [], motion: [], hover: [], unstyled: [],
  };
  const wants = (name) => report.passes === "all" || report.passes.split(",").map((p) => p.trim()).includes(name);
  for (const [density, path_, vp] of [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]]) {
    for (const theme of ['light', 'dark']) {
      // THE PAGE MUST BE RENDERED FOR *EITHER* PASS. The focus block below lives in this loop, so
      // --passes=focus used to run it ZERO times: the mode list was empty, no page was loaded, no Tab
      // was pressed, and the report came back focus: [] — clean, and clean because nothing ran. The
      // full sweep hid it, since all includes pages. Same defect as the ones this suite keeps
      // finding in its own checks, this time in the wiring between two of them.
      const needsPage = wants("pages") || wants("focus") || wants("timing") || wants("hover");
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
          report.surfaces.push({ density, theme, mode: mode_, page: label, ...(await page.evaluate(SURFACE)) });
          report.names.push({ density, theme, mode: mode_, page: label, ...(await page.evaluate(NAMES)) });
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
  if (wants("pages")) {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('http://vale.test/desktop/?theme=light&mode=relaxed&sessions=0&cb=' + stamp, { waitUntil: 'load' });
    await page.evaluate(() => { try { localStorage.setItem('valeGettingStarted', '1'); } catch (e) {} });
    await page.reload({ waitUntil: 'load' });
    await page.waitForTimeout(2000);
    const rows = await page.evaluate(PROBE);
    for (const row of rows) report.rows.push({ ...row, density: 'desktop', theme: 'light', mode: 'relaxed', page: 'Desktop-empty' });
    report.surfaces.push({ density: 'desktop', theme: 'light', mode: 'relaxed', page: 'Desktop-empty', ...(await page.evaluate(SURFACE)) });
    report.names.push({ density: 'desktop', theme: 'light', mode: 'relaxed', page: 'Desktop-empty', ...(await page.evaluate(NAMES)) });
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
            underAA.push(r.sel + ' "' + String(r.text).slice(0, 16) + '" ' + r.cr + '<' + need);
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
  fs.writeFileSync('C:\\\\ProgramData\\\\Vale\\\\pwout\\\\design-sweep.json', JSON.stringify(report));
  console.log(JSON.stringify({ rows: report.rows.length, surfaces: report.surfaces.length, names: report.names.length }));
  await close();
})().catch((e) => { console.error('FATAL', e.message); process.exit(1); });`;
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
      serial: "a session-kind modifier; the element is painted by its [data-kind] rule",
      warn:
        "the boot chip's tone modifier. The BASE rule paints it — .boot-mark is the warn triangle and .boot-mark.info is the exception — so the name needs no rule of its own. Round 121 followed it anyway, and found a real defect behind it: the triangle used --state-warn, which measures 2.80 on the dark chip surface against the 3:1 a graphic needs. Fixed to --warn-ink (6.45 / 8.76).",
    },
    // THIS HARNESS'S OWN BLIND SPOT, named rather than filtered silently. `#tabs` measures ~0-185px
    // in a plain browser and 211px on the device, so the tab strip's children report as overflowing
    // containers here and nowhere else; and the 320px document scroll is the SAME artifact (round 50
    // traced every offending element to a tab inside `#tabs`). A real defect in the strip would have
    // to be judged on the device, which is why this exemption is narrow and printed on every run.
    ignore: [
      { match: /overflow — div\.tabrow/, reason: "panel-density strip widths are a harness artifact (0-185px here, 211px on the device)" },
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
    {
      match: /^span\.approval-grant$/,
      reason: "the grant chip's outline delimits the chip; its meaning is its text (contrast-fixed for this chip already) and its dot",
    },
  ];
  const waived = [];
  for (const r of failures(report.rows)) {
    const why = DECORATIVE.find((d) => d.match.test(String(r.sel)));
    if (why) {
      waived.push(`${r.sel} ${r.cr} — ${why.reason}`);
      continue;
    }
    if (findings.length < 10 + coverage.length) {
      findings.unshift(`${r.cr} ${r.density}/${r.page} ${r.sel} "${String(r.text).slice(0, 24)}"`);
    }
  }
  console.log(reportSummary("panel", report));
  if (coverage.length) console.error(`\n${coverage.join("\n")}`);
  if (unmeasurable(report.rows).length) console.log(`note: ${unmeasurable(report.rows).length} node(s) unmeasurable`);
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
  process.stdout.write(browserScript());
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
