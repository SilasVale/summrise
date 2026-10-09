#!/usr/bin/env node
// panel-design-sweep — run every panel design check in one pass, and judge it.
//
// THE CHECKS ARE THE SHARED CORE'S (`lib/design-sweep.mjs`): headings, landmarks, overflow/clipping/
// slivers, accessible names, reflow, focus. This file adds only what is the PANEL's own — the
// harness it boots, the densities and themes, the approval mode, and boot/interaction timing — and
// its measurements are forwarded to the same judge every other UI uses.
//
// ONE MODE, because the browser lives on a device (the console MCP owns it):
//
//   node agent/scripts/panel-design-sweep.mjs --emit --passes=pages,hover > /tmp/sweep.js
//     writes the `browser_run_script` payload; run it on the device, save the JSON.
//
// THE JUDGE IS RUST NOW (`agent/sweep-judge/`), and it refuses a report that is missing any pass the
// caller said it wanted:
//
//   cargo run --quiet --manifest-path agent/Cargo.toml -p summrise-sweep-judge -- \
//     --tool panel --expect=pages,hover <report.json>
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
//     4. upload that report, curl it down, and judge it with `summrise-sweep-judge --tool panel` — a bare
//        judge reports the div.tabrow artifacts as findings, which is what they are not, so the panel's own
//        options (its `implicitStates`, its floors) are what make the verdict the panel's.
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
//   * **THE SETTINGS REACHABILITY HINT — NAMED, BECAUSE IT IS THE ONE PARAGRAPH ON THAT PAGE THIS SWEEP
//     CANNOT RENDER, AND IT WAS THE WORST LINE ON THE PAGE.** `/api/status` is the only place the panel
//     learns `host` and `relay.configured`; the stub's `/api/status` answers without either, so `host` is
//     absent, the hint does not render AT ALL in the harness, and the `prose` axis therefore measures every
//     Settings paragraph EXCEPT the one an operator with a loopback bind and no relay actually reads. That
//     state is not hypothetical: it is what every installer defaults to (`host: 127.0.0.1`, no
//     `server.relay_url`), and it was measured on a live device instead — which is how the sentence was found
//     running the full width of the card while the `muted` paragraphs beside it wrapped at their cap. Until
//     the fixture exists (`host`, `config_path` and a `relay` with `configured: false` on the stub's
//     `/api/status`), the honest reading of a clean prose axis is that it covers that page MINUS this state,
//     and a sentence may grow past `proseFloor` there with every gate green.
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { PROBE_SOURCE } from "./lib/contrast-probe.mjs";
import { marksProbe, surfaceProbe, namesProbe, reflowProbe, UNSTYLED_SOURCE, focusPass, motionPass, ackNotes, pressPass, discoverPressTargets, revealPass, ackPass, idlePass, TARGETS_SOURCE, THEME_SOURCE, diag, pressDelta } from "./lib/design-sweep.mjs";
import { bundleSweep, piecesModule } from "./lib/sweep-bundle.mjs";
import { sweepPlan } from "./lib/sweep-plan.mjs";

const mode = process.argv[2];
/** `--passes=pages,hover` limits the emitted script; the default is everything. Recorded in the report
 *  so the judge can refuse a partial one — the same rule as the audit's exit codes: a run that did not
 *  measure something must not look like a run that measured it and found nothing.
 *
 *  THE VALUE IS PASSED THROUGH UNTOUCHED (`null` when the flag is absent), because the Rust plan owns
 *  what it means: the panel defaults an absent flag to `all` and treats an EMPTY one as "want
 *  nothing", and both are reachable from a caller. */
const PASSES_ARG = (() => {
  const found = process.argv.find((a) => a.startsWith("--passes="));
  return found === undefined ? null : found.slice("--passes=".length);
})();

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
  // THE PLAN IS RUST AND IT IS COMPUTED HERE (slice 1 of landing 2b). Everything the payload used to
  // decide about itself — which surfaces, in what order, which passes on each, and the caps — comes
  // from the binary at EMIT time and travels into the bundle as data.
  const plan = sweepPlan("panel", PASSES_ARG);
  const { code } = bundleSweep({
    entry: "panel-run.cjs",
    modules: {
      "panel-run.cjs": readFileSync(join(HERE, "lib", "sweep", "panel-run.cjs"), "utf8"),
      "plan-runtime.cjs": readFileSync(join(HERE, "lib", "sweep", "plan-runtime.cjs"), "utf8"),
      "pieces.cjs": piecesSource(plan),
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
function piecesSource(plan) {
  // ONE PIECES GENERATOR, IN THE ASSEMBLER (round 272). This function used to spell out the quoting rule itself —
  // functions by `.toString()`, everything else by JSON — in four different emitters. What is left is the facts.
  return piecesModule({
    // THE PLAN, as data: `wants`, the surface list in visit order, the caps.
    plan,
    config: {
      harnessPath: DEFAULT_HARNESS_PATH,
      selector: "#root",
      reportPath: DEFAULT_REPORT_PATH,
      expectedHarnessBuild: HARNESS_STAMP,
      passes: plan.passes,
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

if (mode === "--emit") {
  // NO SUBSTRING GUARD AND NO HAND-COPIED PARSE CHECK: `bundleSweep` compiled this before returning it, and the
  // helpers are in scope because the payload and the passes meet in one module body — not because a list here names
  // them. Two mechanisms became the assembler's contract, which every emitter now shares.
  process.stdout.write(browserScript());
} else {
  console.error(readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n").slice(1, 20).join("\n"));
  console.error("\nusage: panel-design-sweep.mjs --emit");
  process.exit(2);
}
