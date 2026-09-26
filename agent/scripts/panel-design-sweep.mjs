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
//     device's `summrise-monitor-change` push and nothing else, so a down target on screen with no push
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
//     2. upload it (curl -T to the relay) and system_file_download it to C:\ProgramData\Summrise\pwout\
//     3. on the device, run it in-process — it drives the browser itself:
//          const code = fs.readFileSync(SRC, 'utf8');
//          new Function('require','module','exports','__dirname','__filename','process','console','Buffer',
//                       'setTimeout','clearTimeout', code)(require, {exports:{}}, {}, dir, SRC, process,
//                       console, Buffer, setTimeout, clearTimeout);
//        It prints its summary and rewrites C:\ProgramData\Summrise\pwout\design-sweep.json (~390 KB).
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
//     only behind `window.summriseEmbedded`, so a plain-browser harness renders an explanation page
//     (measured, round 45);
//   * the panel-density tab strip's own width: `#tabs` measures ~0px in the harness and 211px on the
//     device, so harness geometry findings pointing at tab children are suspect;
//   * a background that is an image (judged by the worst-BASE rule in the probe suite instead);
//   * any state the harness fixture cannot produce. Add the fixture, or say the state is unmeasured.
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { failures, unmeasurable, PROBE_SOURCE } from "./lib/contrast-probe.mjs";
import { DECORATIVE_WAIVERS, markCoverageNotes, marksProbe, surfaceProbe, namesProbe, reflowProbe, judgeReport, reportSummary, UNSTYLED_SOURCE, focusPass, motionPass, ackNotes, pressPass, discoverPressTargets, revealPass, ackPass, idlePass, TARGETS_SOURCE, THEME_SOURCE, diag, pressDelta } from "./lib/design-sweep.mjs";
import { bundleSweep, piecesModule } from "./lib/sweep-bundle.mjs";

const mode = process.argv[2];
/** `--passes=pages,hover` limits the emitted script; the default is everything. Recorded in the report
 *  so the judge can refuse a partial one — the same rule as the audit's exit codes: a run that did not
 *  measure something must not look like a run that measured it and found nothing. */
const PASSES = (process.argv.find((a) => a.startsWith("--passes=")) || "--passes=all").slice("--passes=".length);

// WHERE THE EMITTED SWEEP READS AND WRITES BY DEFAULT — the device's paths, and now the EMITTER's values rather than
// text inside a template literal: they were written with four backslashes there because one level was eaten on the way
// into the emitted file (round 267 removed that level, and the landing's first migration caught the same class of
// over-escaping as a real path bug).
const DEFAULT_HARNESS_PATH = "C:\\ProgramData\\Summrise\\pwout\\panel-harness.html";
const DEFAULT_REPORT_PATH = "C:\\ProgramData\\Summrise\\pwout\\design-sweep.json";
const HERE = fileURLToPath(new URL(".", import.meta.url));

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
// AND IT CAN BE HANDED OVER (round 237): on a device there is no ../resources/panel to read, so the value comes from the
// caller — the same variable the harness emitter reads, which is what keeps their two identities equal by construction rather
// than by both happening to read the same file. Inlined at emit time, so this is a decision the EMITTING environment makes.
const HARNESS_STAMP = process.env.SUMMRISE_HARNESS_STAMP || (() => {
  try {
    const css = readFileSync(new URL("../resources/panel/panel.css", import.meta.url));
    return css.length + "-" + createHash("sha256").update(css).digest("hex").slice(0, 12);
  } catch (e) { return "(unreadable)"; }
})();
function browserScript() {
  // THE PAYLOAD IS A REAL MODULE NOW (round 267). It was this file's own template literal — 1012 lines, 62% of the
  // file, with nineteen interpolations — which is why a backtick in any of the nine passes, the probe, the page checks
  // or the diag helper could end THIS file mid-parse (53 recorded incidents), why the Windows defaults were written
  // with four backslashes to survive one level of escaping, and why `assertEmbedded` existed to check by substring
  // that a borrowed helper had also been spliced. All of that is gone: lib/sweep/panel-run.cjs is ordinary code, the
  // run-varying values arrive as the generated pieces module beside it, the assembler resolves the payload's own
  // requires, and it COMPILES what it returns.
  const { code } = bundleSweep({
    entry: "panel-run.cjs",
    modules: {
      "panel-run.cjs": readFileSync(join(HERE, "lib", "sweep", "panel-run.cjs"), "utf8"),
      "pieces.cjs": piecesSource(),
    },
  });
  return code;
}

/** The probe snippet a host constant carries, as the STRING the page evaluates. MOTION and TIMING were written as
 *  `const MOTION = \`(() => {...})()\`;` — a snippet that DEFINES a const whose value is the probe — so the old
 *  emitter spliced the snippet as code and the payload read `MOTION` afterwards. One level of that is removable now:
 *  the pieces module carries the probe text and the payload binds it to the same name. A snippet that does not match
 *  THROWS, because binding undefined would make the pass that uses it measure nothing. */
function probeOf(snippet, name) {
  const m = new RegExp("const " + name + " = `([\\s\\S]*)`;\\s*$").exec(snippet);
  if (!m) {
    throw new Error(
      name + ": expected the snippet to be 'const " + name + " = <backtick>...<backtick>;' — the emitted payload " +
        "would bind undefined and the pass that uses it would measure nothing",
    );
  }
  return m[1];
}

/** The run-varying pieces, as a module (the same shape and the same reasoning as the landing's, round 266). */
function piecesSource() {
  // ONE PIECES GENERATOR, IN THE ASSEMBLER (round 272). This function used to spell out the quoting rule itself —
  // functions by `.toString()`, everything else by JSON — in four different emitters. What is left is the facts.
  return piecesModule({
    config: {
      harnessPath: DEFAULT_HARNESS_PATH,
      selector: "#root",
      reportPath: DEFAULT_REPORT_PATH,
      expectedHarnessBuild: HARNESS_STAMP,
      passes: PASSES,
    },
    probe: PROBE_SOURCE,
    unstyled: UNSTYLED_SOURCE,
    targets: TARGETS_SOURCE,
    theme: THEME_SOURCE,
    checks: { SURFACE: surfaceProbe, NAMES: namesProbe, REFLOW: reflowProbe },
    marks: marksProbe,
    // THE PANEL'S TWO EXTRA PROBES. MOTION and TIMING are written as snippets that DECLARE a const holding the probe
    // text (they predate this seam); probeOf() reads the text out of them and THROWS if the shape changes, because a
    // silent undefined here would make the motion and timing axes measure nothing at all.
    motion: probeOf(MOTION, "MOTION"),
    timing: probeOf(TIMING, "TIMING"),
    diag,
    passes: { focusPass, pressDelta, discoverPressTargets, pressPass, revealPass, ackPass, ackNotes, idlePass, motionPass },
  });
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
    // PROSE HAS A MEASURE, AND THIS ROUND MEASURED WHETHER IT DOES (round 265). A line is read by its return:
    // the panel's own ledes cap at 52/56/66/72ch and this sheet has said so in five places for as long as they
    // have existed — but nothing MEASURED it, so the Settings page rendered twelve paragraphs as SINGLE lines of
    // 93-206 characters at 1440px, in both densities, three blocks from a History lede that wraps at 73.
    // THE FLOOR IS 90, and it is the defect class that chose it: the twelve offenders start at 93, the panel's own
    // capped ledes render 70-73, and 90 leaves a fifth of headroom over the rule (66ch) rather than encoding the
    // rule itself — a `ch` cap and a measured `cpl` are different units (the cap is the box, `cpl` is the average
    // glyph), and judging the unit the READER experiences is the point. The console and the landing carry the same
    // numbers in their reports and are NOT failed by this floor: neither has been measured on this axis, and a
    // threshold somebody else picked is not a finding about them.
    proseFloor: 90,
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
  // A MEASUREMENT THAT FOUND NOTHING TO MEASURE IS NOT A PASS (round 265). The prose axis is only as good as the
  // text blocks it matched: a selector change, a probe that stopped counting, or a harness that rendered an empty
  // page would ALL report "no long lines", and this suite has caught exactly that shape in its own checks before
  // (a pass that ran nothing, an axis that measured zero). The six live pages alone produce dozens of blocks, so
  // the floor is deliberately far below the real number: this guards the INSTRUMENT, not the panel.
  // SCOPED TO THE RUN THAT PRODUCES SURFACES: a report from `--passes=focus` has none, and that is the coverage
  // clause's business (it fails a run missing a pass the caller asked for) rather than this axis's — a floor that
  // fired there would be a finding about a page nobody rendered.
  {
    const pagesRan = report.passes === "all" || String(report.passes || "").split(",").map((p) => p.trim()).includes("pages");
    const measured = (report.surfaces || []).reduce((n, s) => n + ((s.measure && s.measure.measured) || 0), 0);
    if (pagesRan && measured < 12) {
      findings.push(
        `the prose-measure axis matched only ${measured} text block(s) across ${(report.surfaces || []).length} surface(s) — a run that measured nothing cannot clear this axis`,
      );
    }
  }
  // DECORATIVE GRAPHICS: drawn to DELIMIT, not to inform. WCAG 1.4.11 applies to a non-text element that
  // CARRIES MEANING; a chip's 1px hairline does not, and this one measures 1.2 against a surface it was
  // never meant to contrast with. Named here rather than silently dropped — the same rule the rest of
  // this suite follows — and the waived rows are PRINTED on every run so the exemption stays visible.
  // THE WAIVERS THEMSELVES LIVE IN THE SHARED LIB (round 265), because the panel's OTHER instrument reads the
  // same rows: the audit reported two of these elements as failures the moment it was made to run, which is a
  // policy disagreement between two tools rather than a defect in the panel. One list, two readers.
  const DECORATIVE = DECORATIVE_WAIVERS;
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
  // IMMEDIATE FEEDBACK HAS A BUDGET (round 19) IS JUDGED IN THE SHARED JUDGE NOW — `judgeReport` owns the clause, and
  // this file's copy was DELETED (round 2 of the standing goal). "Pressed and acknowledged states fire on the EVENT,
  // not on the network, inside a stated budget": the budget is 100ms, well under any round trip and about six frames,
  // and the measurement is the gap between the press and the first visible acknowledgement against a fixture that
  // delays every reply by 900ms. The clause sat HERE, which is why the panel's rows were judged and the console's —
  // the same array, a third of that sweep's runtime — were judged by nothing at all. One report shape, one clause.

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
  // NO SUBSTRING GUARD AND NO HAND-COPIED PARSE CHECK: `bundleSweep` compiled this before returning it, and the
  // helpers are in scope because the payload and the passes meet in one module body — not because a list here names
  // them. Two mechanisms became the assembler's contract, which every emitter now shares.
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
