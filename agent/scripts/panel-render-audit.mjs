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
import { createHash } from "node:crypto";

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
  // `idle_ms` IS THE PER-SESSION FACT THE MARK LANGUAGE READS (lib/liveness.ts): the agent's own
  // `last_output.elapsed()`, and a session inside the window is WORKING — the halo. THE FIXTURE HAD NO SUCH FIELD
  // AT ALL, so every sweep this harness ever produced photographed a panel where no session could be working:
  // the marquee silhouette of the whole language was invisible to the gate that checks silhouette coverage.
  // 900 ms here, and the quiet seeds below, so the strip carries one halo and two rings.
  idle_ms: 900,
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
  // THE SESSION ROWS (restored round 115 — the same deleted block took this as well, so the stub
  // answered terminal_list with an undefined result and the panel showed "Sessions unavailable —
  // reconnecting" no matter what else was fixed). Same shape as the single SESSION row above, which is
  // the shape agent/tests/fixtures/session-row.json pins from both ends.
  // THE STREAM OPENS NOW, AND THIS NOTE SAID OTHERWISE FOR SIXTY ROUNDS (corrected round 86). It was written in
  // round 115, when window.EventSource was a NO-OP and the panel's push never arrived — so every surface really was
  // measured in its "Connection lost - reconnecting" state. Round 156 put the branch in (see /api/events/term
  // below, which serves one empty frame and closes) and the panel has rendered CONNECTED since; the paragraph that
  // said "NEVER opens" stayed, and it is exactly the kind of note that makes a reader distrust a working fixture or
  // "fix" what is not broken. What replaced it is a CHECK, not a sentence: the harness publishes window.__sse, the
  // panel sweep reads it per surface, and the shared judge FAILS a surface that should be connected and was not.
  // (window.EventSource is still a no-op and that is still correct: the app fetches /api/events/term and reads a
  // stream rather than using EventSource.)
  //
  // WHAT THE PANEL SHOWS BECAUSE OF IT, traced to the line (round 154): PanelApp defines connected as
  // props.sseState === "connected" — the SSE state, NOT the boot connection — and the sidebar message
  // "Sessions unavailable — reconnecting…" is therefore CORRECT for this harness rather than a bug. The
  // app does not use EventSource at all: useSSE fetches /api/events/term and reads a stream, flipping to
  // connected on any complete frame, even an empty one.
  //
  // THE ATTEMPT THAT WAS REVERTED, AND THEN MADE TO WORK (corrected round 87). Round 154 served one empty frame
  // from /api/events/term and put the app on the CONNECT SCREEN (connForm true, 5 text rows) whether the stream
  // never ended or ended right after the frame; the cause was not found and the branch came out. Round 156 put it
  // back, read the page errors instead of guessing, and it has served the connected state ever since — so the
  // sentence that ended this paragraph ("with the stream shut, every panel measurement this harness has ever
  // produced was taken in a reconnecting state") describes round 154, not this file. The history is kept because
  // the symptom is worth recognising; the CONCLUSION is not, and a reader who takes it for the present tense will
  // distrust a fixture that works.
  // ?sessions=N — and ZERO IS THE POINT. This list was a fixed three, so the panel's EMPTY state (a fresh
  // install, a device with nothing open) could not be rendered at all, and therefore had never been
  // measured by anything. That is the same shape as rounds 148-149's findings: a real state no sweep
  // visits. The three seeds below reproduce the old list exactly when no parameter is given.
  var liveCount = P.has('sessions') ? Math.max(0, parseInt(P.get('sessions'), 10) || 0) : 3;
  // ── HOW TO REACH THE FOURTH SILHOUETTE ('off', a tombstone), BECAUSE NO PARAMETER CAN ────────────────────────
  // A closed session is CLIENT state, not wire state: the panel tombstones a tab when the operator closes it
  // (useSessions.closeSession) or when the device's list stops naming a session it had. So '?closed=1' cannot
  // exist — and the page sweep, which photographs pages and never presses, has never photographed 'off'.
  // Measured recipe (round 10 of the standing goal), and it has a TIMING TRAP worth keeping:
  //   1. click '.tab .tab-close'            (arms the two-step close)
  //   2. click the confirm's Close button   ('/api/tools/terminal_close' is stubbed, so it succeeds)
  //   3. READ BETWEEN ~200ms AND ~1.4s: the tab's background takes 195ms to settle out of the ACTIVE fill (the
  //      tab's own 0.15s background transition), and at ~1.4s the harness's still-live session list REVIVES the
  //      tombstone by round 245's rule — a live reappearance means the session is real.
  // A probe that reads immediately measures the transition, not the state: it reports the 'off' mark sitting on
  // the active tab's accent fill at 1.00:1, which looks exactly like the round-9 defect and is not one.
  var SESSION_SEEDS = [
    {},
    // QUIET BUT BUSY, ON PURPOSE. command_running is the device's own answer — the execute wait-loop sets it
    // around the command it is waiting for — and it is the ONLY way a SILENT command can read as working: this
    // session has been quiet for 45s, well past the recency window, so a panel that shows it working is showing
    // the device fact and not the inference. Without it in the fixture the state exists on the wire and nowhere a
    // sweep can photograph it, which is the lesson round 9 learned about idle_ms. (No backticks in here: this
    // text lives INSIDE the emitted template literal, and a stray one ends it — the 37th time.)
    { label: 'serial:COM4', kind: 'serial', idle_ms: 45_000, command_running: true, held_by_human: false, pending_approval: null, approval_required: false },
    { label: 'stc@192.168.1.1', kind: 'ssh', idle_ms: 120_000, held_by_human: false, pending_approval: null },
  ];
  // SESSIONS THE OPERATOR CLOSED, and they STAY closed (round 36 of the standing goal). A tombstone is client
  // state: the panel draws one when the device's list stops naming a session it had, and REVIVES it if the name
  // comes back (round 245 — a live reappearance means the session is real). With a static list that meant the
  // tombstone lived for about a second, so the fourth silhouette — the DASHED RING, whose whole design is the
  // decision that 'off' is a dash and not a fade — could only be photographed inside a timing window: click the
  // tab's x, click the confirm's Close, read between 200ms and 1.4s. Now terminal_close removes the session from
  // every later list answer, so the state is a URL plus two clicks and it stays put.
  var CLOSED = [];
  var SESSIONS = [];
  for (var si = 0; si < liveCount; si++) {
    SESSIONS.push(Object.assign({}, SESSION, SESSION_SEEDS[si % 3], { id: 'term-audit-' + si }));
  }
  // RESTORED WITH THE HEADER (round 115): the same round-112 edit that deleted the fetch function's
  // opening also deleted these two lines, so the header referenced an undeclared FAIL and the counter
  // every measurement reads was never created. The emitted stub threw "FAIL is not defined" on the
  // first request, which is why the app showed "reconnecting" and why window.__calls was undefined.
  // ?held=1 — THE STATE THE PANEL IS MOST CAREFUL ABOUT, AND ONE NO SURFACE HAS EVER RENDERED (round 89).
  // held_by_human is SERVER-OWNED: SessionControl reads it off the session record and refuses to flip the button
  // before the server agrees, because another client taking the session has to show up here too. Every seed above
  // sets it false, so the HUMAN state of the .sc-dot mark — a solid fill against the ai state's inset ring — has
  // never been photographed, and neither has the button's .held variant.
  // ONLY THE FIRST SESSION, so both states of the family are on one page: the marks probe compares states WITHIN a
  // family and can only see a collision between two states that are both rendered.
  // ?goal=none — THE COMMON CASE, AND ONE NO SURFACE HAS EVER RENDERED (round 90). GoalBar says it plainly in its
  // own comment: "nothing stated -> a QUIET affordance. An unset goal is normal (most sessions ...)". The fixture
  // above sets a goal on EVERY session, so the affordance — a dashed-bordered button whose only content is a text
  // node, with no .goal-text span inside it — has never been on screen, while the state it renders instead has been
  // measured on every page since the harness was written.
  var NO_GOAL = P.get('goal') === 'none';
  if (NO_GOAL) SESSION.goal = null;

  var HELD = P.get('held') === '1';
  if (HELD && SESSIONS.length) SESSIONS[0].held_by_human = true;

  // ?exitfail=1 — THE DEVICE'S OWN COMMAND OUTCOME, WORN ON A ROW (round 96). The wire carries
  // last_exit_code now (agent/src/tools/terminal/mod.rs), and the panel draws a chip for a NON-ZERO code
  // only. Every seed above reports no code at all, so without this flag the chip has no surface anywhere —
  // the lesson rounds 88-92 paid for four times running: A STATE NO SURFACE RENDERS IS A STATE NO SURFACE
  // MEASURES. The chip's three states are all on one page: the first session failed (Some(1) -> chip), the
  // rest report nothing (absent -> no chip), and ?exitok=1 below is the third.
  // (No backticks: this comment lives inside the emitted template literal — 51st time, caught by the emit.)
  // ON THE SSH SEED (index 2), NOT THE FIRST SESSION, and that is what puts FOUR states on one page: with
  // mode=pending the first seed is the one holding the question (WAITING outranks everything) and the second is
  // the busy serial session (WORKING), so a failure on either of those would be invisible. Index 2 is the quiet
  // ssh row — idle by every other measure — which is exactly the session a failure mark has to describe.
  var EXIT_FAIL = P.get('exitfail') === '1';
  if (EXIT_FAIL && SESSIONS.length > 2) SESSIONS[2].last_exit_code = 1;
  // THE THIRD STATE, on its own surface because it is the one a surface must not get wrong: exit ZERO is an
  // answer, and it must render as EMPTY rather than as a chip saying "exit 0".
  var EXIT_OK = P.get('exitok') === '1';
  if (EXIT_OK && SESSIONS.length > 2) SESSIONS[2].last_exit_code = 0;
  // ?exitfail=active — THE SAME STATE ON THE SESSION THE OPERATOR IS LOOKING AT (round 98). The failure lands on the
  // FIRST session, which is the ACTIVE one, so the mark is drawn on the accent-filled active tab. That combination
  // is where the ink finally broke (--danger-on-soft draws 2.16:1 there in dark) and it had NO SURFACE for two
  // rounds: every other flag plants the failure on a quiet row. Use it with mode=idle, because waiting and working
  // outrank a failure and would hide it.
  var EXIT_FAIL_ACTIVE = P.get('exitfail') === 'active';
  if (EXIT_FAIL_ACTIVE && SESSIONS.length) {
    // AND IT HAS TO BE QUIET, or the state is invisible: EVERY seed above reports idle_ms 900 — "just produced
    // output" — which reads as WORKING, and working outranks a failure by design. The first run of this flag
    // rendered mark[working,idle] and no failed mark at all, which is the state model behaving correctly and the
    // fixture failing to express what it claimed. 60s of silence puts the failure on top, where a person can see it.
    SESSIONS[0].idle_ms = 60_000;
    SESSIONS[0].last_exit_code = 1;
  }

  var FAIL = P.get('fail') === '1';
  window.__calls = [];
  // DID THE STREAM OPEN? The panel's connected state comes from a COMPLETE FRAME on /api/events/term (the app
  // fetches a stream rather than using EventSource), so "the harness delivered the push" is a fact only this file
  // knows. It is published here for the sweep to assert, together with FAIL, because the failure surfaces reject
  // every /api/ call on purpose and are SUPPOSED to read as reconnecting.
  window.__sse = { opened: false, fail: FAIL };
  var realFetch = window.fetch.bind(window);
  // THE RESPONSE HELPER, which every fixture guard below calls and which the same edit deleted. Without
  // it every guard threw "J is not defined" on the first request: the app's calls failed, the cards said
  // "unavailable" and "reconnecting", and window.__calls still grew because the counter pushes BEFORE the
  // guards run — which is why the count looked healthy while nothing was ever served.
  function J(obj) {
    // EVERY JSON FIXTURE CLAIMS THE DEVICE ANSWERED, AND IT CANNOT FORGET TO (round 100). Several readers require
    // ok === true and treat its absence as a FAILED READ — useBootHistory, useAgentVitals, the update card, the
    // session list — so a stub that omitted it made the panel BLAME THE DEVICE for a question it had answered:
    // the update card (round 110's note), the monitors (round 100), the restart history (round 99). Three rounds,
    // one field. The rule was written down here and nothing enforced it, which is why it happened again.
    // An OBJECT body gets the envelope unless it brings its own ok (a stub that MEANS to express a failure says
    // ok:false, and this leaves it alone); an ARRAY body is passed through untouched, because a reader expecting
    // a bare list would break on an object.
    if (obj && typeof obj === 'object' && !Array.isArray(obj) && !('ok' in obj)) {
      obj = Object.assign({ ok: true }, obj);
    }
    return new Response(JSON.stringify(obj), { status: 200, headers: { 'content-type': 'application/json' } });
  }
  window.fetch = function(url, init){
    var u = String(url);
    if (FAIL && u.indexOf('/api/') >= 0) return Promise.reject(new TypeError('Failed to fetch'));
  // THE APPROVAL CLOCK, three states a person actually sees (round 46). The default fixture carries
  // expires_in_ms: 47000, which the gate reads as URGENT (under its one-minute threshold) — so the
  // alarmed state was the only one any sweep had ever rendered. 'relaxed' is a question with time
  // left, 'expired' is the zero-second rule: the device has retired it and nothing can be answered.
  // EVERY FIXTURE BELOW NEEDS ok:true — see the note on the update card's envelope. Written here
  // because this is where fixtures are added, and the omission has cost three rounds.
  //
  // THE UPDATE CARD, WHICH HAD NEVER RENDERED A STATE AT ALL (round 110). /api/update was served by
  // nothing, so the card only ever showed its empty "unknown" face — including the APPLYING state,
  // which is the one an operator stares at during a release. ?busy=1 is that state.
  //
  // SOLVED, AND THE ANSWER WAS SCOPE (round 112). The card renders both of its states now:
  //
  //   available  running 1.2.403 | 1.2.433 available | [Update to 1.2.433]   data-available=yes
  //              measured 16.69 light / 14 dark for the version line, 7.45 / 7.08 for the latest, and
  //              the action button 4.83 light / 6.71 dark — all above AA
  //   applying   running 1.2.403 | latest is 1.2.403 | "An update is already in flight on this device
  //              — a second one would race it."   data-available=no, NO BUTTON (absent, not disabled)
  //              measured 16.69 / 14 and 7.66 / 6.54
  //
  // WHAT WAS ACTUALLY WRONG, after three rounds: this block sat OUTSIDE window.fetch — inserted above
  // it, where u does not exist — so it threw "u is not defined" while the stub was being DEFINED, and
  // the route was never served. The card's message, "The device did not answer, so its update state
  // could not be read", was literally true the whole time. I read it as a bug report about the card.
  //
  // TWO INSTRUMENT LESSONS, both paid for here:
  //   * a page-level fetch does NOT speak for the app. Probing the route that way returned HTML for
  //     /api/update AND /api/status, and /api/status demonstrably works — so the probe was measuring
  //     the page.route handler, not the stub.
  //   * A HARNESS EDIT NEEDS A BOOT CHECK. node --check and a successful emit only prove the FILE
  //     parses; they say nothing about the inlined script. A stub that throws at definition time leaves
  //     a page that renders its fallback, and every measurement afterwards describes a page that never
  //     ran. Assert pageErrors is empty and #root has children BEFORE believing any number below.
  //
  // A "narrow the note" edit also deleted this block once (round 111), between two comment markers.
  // When editing between markers, read what sits between them.
  if (u.indexOf('/api/update') >= 0) {
    var busy = P.get('busy') === '1';
    // ok:true IS REQUIRED BY THE HOOK. Every hook that reads through callApi checks it first, and a
    // fixture without it produces the card's failure face while the payload looks right. (The three
    // rounds this cost were NOT this, though — they were the code block below being deleted by a
    // comment-spanning edit. Keep the guard and the payload together, and never edit between two
    // comment markers without checking what sits between them.)
    return Promise.resolve(J({
      ok: true,
      current: '1.2.403',
      channel: 'stable',
      latest: busy ? '1.2.403' : '1.2.433',
      update_available: !busy,
      pinned_to: null,
      busy: busy,
      error: null,
      // WHEN THE DEVICE LAST ASKED (2026-09-21). update_status has reported checked_at since it was written, and
      // the panel dropped it — so the card stated a version with no age on it while the device answers from a
      // 30-second cache. A state no fixture renders is a state no sweep measures, so the stub carries it now: the
      // age line is drawn (and contrast-measured) on every Settings surface.
      //
      // FIXED, NOT RELATIVE: the fixture is a page, and a page whose numbers move between the harness and the
      // report would make two runs of the same surface disagree. Two minutes ago is recent enough to read in
      // seconds-vs-minutes terms and stable enough to compare.
      checked_at: Date.now() - 120000,
    }));
  }

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
  // THE DEVICE AREA, POPULATED (round 114). Three cards the panel has only ever rendered EMPTY: the
  // catch-all answers {ok:true} for any unrouted path, so /api/status, /api/vitals/history and
  // /api/boots each produced a card with nothing in it — through every sweep, including the rounds that
  // built fixtures for their shapes and never served them here. The data below is the shape those
  // fixtures pin, which is the point: status keys with the conditional fields present, vitals samples
  // OLDEST FIRST with ts_ms in milliseconds, and boot records NEWEST FIRST with the kind vocabulary.
  if (u.indexOf('/api/status') >= 0) {
    return Promise.resolve(J({
      ok: true, version: '1.2.433', port: 18080, uptime_secs: 5412, live_sessions: liveCount, serial_ports: ['COM4'],
      release: '1.2.433', cpu_pct: 12.5, mem_pct: 41.7, mem_total_mb: 16384, pending_approvals: 1,
      last_boot: '2026-09-13 04:12:03 +08:00 - unexpected exit', last_boot_kind: 'crashed',
    }));
  }
  if (u.indexOf('/api/vitals/history') >= 0) {
    var samples = [];
    for (var i = 0; i < 40; i++) {
      samples.push({
        ts_ms: 1789000000000 + i * 30000,
        cpu_pct: Math.round((8 + 30 * Math.abs(Math.sin(i / 4))) * 10) / 10,
        mem_pct: Math.round((38 + 6 * Math.abs(Math.cos(i / 6))) * 10) / 10,
        mem_total_mb: 16384,
      });
    }
    // ok:true AGAIN — the fourth fixture here to omit it, and the harness note above has warned about it
    // since round 101. useVitalsSeries checks j?.ok !== true first and renders "The device did not
    // answer, so its vitals could not be read" otherwise, which reads like a product bug and is not one.
    return Promise.resolve(J({ ok: true, interval_secs: 30, span_secs: 1200, samples: samples }));
  }
  if (u.indexOf('/api/boots') >= 0) {
    // THE ENVELOPE MATTERS, AND THIS STUB WAS MISSING IT (round 99) — the same lesson the monitors stub below
    // records from round 100, one endpoint over. useBootHistory requires ok === true and treats anything else
    // as a FAILED read, so a body that looked perfectly good here made the Restarts card render "The device did not
    // answer, so its restart history could not be read" on EVERY Settings surface — a false claim about the device,
    // photographed and judged clean for as long as the surface has existed, with the card's real content (the
    // summary line and the crash rows) measured by nothing at all. (No backticks: emitted template literal.)
    return Promise.resolve(J({ ok: true, boots: [
      { ts_ms: 1789000000000, kind: 'crashed', detail: '2026-09-13 04:12:03 +08:00 - unexpected exit', uptime_secs: 5412, gap_secs: 1, release: '1.2.433' },
      { ts_ms: 1788900000000, kind: 'replaced', detail: '2026-09-12 09:00:00 +08:00 - replaced by vale update', uptime_secs: 0, gap_secs: 1, release: '1.2.433' },
      { ts_ms: 1788800000000, kind: 'first-run', detail: '2026-09-11 08:00:00 +08:00 - first run', release: null },
    ] }));
  }
  // /api/operation — THE MERGED ACTIVITY FEED, WHICH NOTHING EVER SERVED (round 102). The History page's second
  // scope ("Runs") reads it through useOperationRuns, and it was stubbed NOWHERE: the generic /api/ branch answered
  // {ok:true}, events was therefore not an array, and the view drew its empty state on every surface that ever
  // showed it. The device serves a merge of the terminal audit trail, the browser feed and the run boundaries
  // (agent/src/web/mod.rs, /api/operation); this mirrors the client's own types (lib/runs.ts: OperationEvent,
  // RunBoundary) so the run strip, the grouping and the per-row states all have something real to draw.
  // (No backticks: emitted template literal.)
  if (u.indexOf('/api/operation') >= 0) {
    var opRows = [];
    var mk = function (source, ts, kind, extra) {
      var row = { source: source, ts_ms: ts, kind: kind };
      for (var k in extra) row[k] = extra[k];
      return row;
    };
    // A run boundary, a command that succeeded, a command that FAILED, and browser work inside the same run —
    // which is what the view exists to put on ONE timeline.
    opRows.push(mk('terminal', 1789000000000, 'run', { run_id: 'run-1' }));
    opRows.push(mk('terminal', 1789000001000, 'command/start', { session: 'term-arch-0', seq: 4, command: 'display ont info 0 1', intent: 'check the ONU before changing it', considered: ['reset the ONU'] }));
    opRows.push(mk('terminal', 1789000001100, 'output', { session: 'term-arch-0', seq: 5, text: 'ONT 0/1 online, VLAN 1' }));
    opRows.push(mk('terminal', 1789000001200, 'command/end', { session: 'term-arch-0', seq: 6, exit_code: 0, duration_ms: 900 }));
    opRows.push(mk('browser', 1789000001400, 'navigate', { script: 'goto', text: 'https://192.168.1.1/' }));
    opRows.push(mk('browser', 1789000001600, 'action', { script: 'click', text: 'Login' }));
    opRows.push(mk('terminal', 1789000002000, 'command/start', { session: 'term-arch-1', seq: 9, command: 'vlan 100', intent: 'apply the change' }));
    opRows.push(mk('terminal', 1789000002200, 'command/end', { session: 'term-arch-1', seq: 10, exit_code: 1, duration_ms: 200 }));
    return Promise.resolve(J({
      cursor_ms: 1789000003000,
      events: opRows,
      runs: [
        { kind: 'run_begin', run_id: 'run-1', ts_ms: 1789000000000, label: 'provision the ONU', goal: 'provision the ONU 0/1 on VLAN 100' },
        { kind: 'run_end', run_id: 'run-1', ts_ms: 1789000002500, outcome: 'done' },
      ],
    }));
  }

  // THE DEVICE'S LOGS — THE FOURTH CARD IN THIS FAMILY TO HAVE NEVER RENDERED ITS REAL STATE (round 100).
  // /api/logs was stubbed by NOTHING, so the DeviceLogsCard drew "The device did not answer, so its logs could not
  // be read" on every Settings surface since it existed — the same false claim the restart card made (round 99) and
  // the monitors card made (round 100), found this time by the sweep's new CLAIM clause rather than by hand. The
  // real card renders a VERDICT derived from vale-update.log's tail (updateDiagnosis's four-way table), a receipt,
  // the directory, and one row per log file with an ABSENT file named as absent. The payload below mirrors
  // api_logs() in agent/src/web/mod.rs: ok, dir, logs[] with name/present/log. (No backticks: emitted template —
  // and this one broke the EMITTER'S OWN MODULE rather than the emitted text, 57th time.)
  if (u.indexOf('/api/logs') >= 0) {
    // ?logs=warn — THE OTHER VERDICT TONE, which the default payload cannot show (round 100). The four-way table
    // from updateDiagnosis gives cli-swap-launched/rust-swap an OK tone and cli-only/never-arrived a WARN one, so a
    // fixture with a receipt AND a start can only ever render OK. This one has the receipt with NO start line: the
    // CLI reached the device and the swap never launched, which is the state an operator investigating a stalled
    // update actually sees.
    var LOGS_WARN = P.get('logs') === 'warn';
    var updateLog = LOGS_WARN
      ? '2026-09-18 15:20:01 update requested 1.2.433 -> 1.2.435'
      : '2026-09-18 15:20:01 update requested 1.2.433 -> 1.2.435' + String.fromCharCode(10) +
        '2026-09-18 15:20:01 update start: swapping in 1.2.435' + String.fromCharCode(10) +
        '2026-09-18 15:20:04 copy ok' + String.fromCharCode(10) +
        '2026-09-18 15:20:06 restarting service';
    return Promise.resolve(J({
      dir: 'C:/ProgramData/Vale/logs',
      logs: [
        { name: 'vale-update.log', present: true, log: updateLog },
        { name: 'agent.log', present: true, log: [
          '2026-09-18 15:20:06 INFO vale_agent: serving on 127.0.0.1:18080',
          '2026-09-18 15:20:07 INFO vale_agent::tunnel: tunnel up',
        ].join(String.fromCharCode(10)) },
        { name: 'startup.log', present: true, log: '2026-09-18 15:20:06 +08:00 - clean start after update' },
        { name: 'vale-mcp.log', present: false, log: '' },
      ],
    }));
  }
  var DOWN = P.get('monitor') === 'down';
  if (u.indexOf('/api/monitors') >= 0 && u.indexOf('/api/monitors/') < 0) {
    var probe = function (i, ok, ms) { return { ts_ms: 1789000000000 + i * 15000, ok: ok, ms: ms }; };
    // THE ENVELOPE MATTERS: the hook requires ok === true and treats anything else as a FAILED read
    // — which is what made this surface render nothing in round 100, with the payload looking
    // perfectly good to me. The device sends it (monitor.rs, snapshot). (No backticks: this text lives
    // inside the emitted template, and the tenth stray one shut --emit down.)
    return Promise.resolve(J({ ok: true, targets: [
      {
        id: 'mon-router', host: '192.168.1.1', port: 22, path: null, expect: null,
        // drops 2 == the device's UNSTABLE_DROPS, so this target renders the FLAPPING chip as well as
        // the down one below it. Both chip states were unmeasured until round 109: earlier probes looked
        // at .monitor-alerts and .monitor-row and never at .monitor-chip.
        summary: { probes: 240, up: 239, down: 1, up_pct: 99.6, up_now: true, since_ms: 1789000000000, drops: 2,
                   latency: { min: 3, avg: 11, max: 88 }, last_status: null, body_ok: null },
        transitions: [{ at_ms: 1788990000000, up: false, lasted_ms: 300000 }],
        series: [probe(0, true, 9), probe(1, true, 12), probe(2, true, 8)],
      },
      {
        id: 'mon-ont', host: '192.168.1.1', port: 8000, path: '/status', expect: 'ONT', path_label: 'http',
        // UP BY DEFAULT, so the FLAPPING chip can render: the component gives DOWN precedence over
        // flapping, so a down target anywhere hides the flapping one entirely (measured round 109 — the
        // two states are mutually exclusive by design). ?monitor=down flips this one back, because
        // round 122 found the consequence: the DOWN row's own border and its down chip had stopped
        // rendering at all, so nothing measured them.
        summary: { probes: 240, up: 239, down: 1, up_pct: 99.6, up_now: !DOWN, since_ms: 1789002000000, drops: DOWN ? 3 : 5,
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
  //
  // AND THE MATCH IS EXACT NOW (round 101), because a BROAD MATCH SHADOWED THE SPECIFIC ONE BELOW: this was
  // u.indexOf('/api/sessions') >= 0, which also matches /api/sessions/<sid> — so the per-session stub (the one
  // that serves the audit trail) was UNREACHABLE, and every request for a session's events got this archive body
  // instead. (58th backtick incident, caught by the emit before the commit.) With ok: true from the envelope change, the reader treated that as a SUCCESSFUL EMPTY READ: the
  // Trajectory and Path views drew "No commands in this session yet" and "No path yet" on every surface that has
  // ever shown them, and the panel's two most information-dense views were measured against an empty page. THE SAME
  // DEFECT CLASS as the last three rounds — a fixture answering a question nobody asked — found this time by
  // clicking a tab no sweep had clicked. (No backticks: emitted template.)
  // NO REGEX AND NO BACKSLASHES, deliberately: three nesting levels (this template, the emitted stub, the
  // browser) eat them, which is the lesson the diag helper records in this same file. A path COMPARISON says
  // exactly what the broad indexOf could not: the route is /api/sessions and nothing beyond it.
  var sessPath = u.split('?')[0];
  if (sessPath.slice(-13) === '/api/sessions') {
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
  // THE FUNCTION HEADER. Round 112's "move the block inside the stub" edit deleted these three lines
  // along with the misplaced fixture — the block between its two comment markers INCLUDED the opening
  // of window.fetch — so from then on every fixture below sat at the stub's top level, u and init
  // did not exist, and the emitted script threw before it could serve anything. The app fell back to the
  // connect screen, and four rounds of measurements described a page that never ran. If this header is
  // ever missing again, the emitted stub will not parse: check it with the boot check in
  // agent/scripts/harness-boot-check.mjs before believing any number.
    var body = (init && init.body) ? String(init.body) : '';
    // Double backslash: this is inside a template literal, where a single \/ collapses to / and the
    // emitted regex becomes /^.*/api// — "Invalid regular expression flags", which killed the WHOLE
    // stub (no token, no sessions, no counter) and looked from the outside like a broken product.
    window.__calls.push(u.replace(/^.*\\/api\\//, '') + ' ' + body);
    if (u.indexOf('/api/tools/terminal_close') >= 0) {
      var closeSid = null;
      try { closeSid = JSON.parse(body || '{}').session_id || null; } catch (e) { closeSid = null; }
      if (closeSid && CLOSED.indexOf(closeSid) < 0) CLOSED.push(closeSid);
      return Promise.resolve(J({ ok: true, result: { closed: !!closeSid } }));
    }
    if (u.indexOf('/api/tools/terminal_list') >= 0) {
      return Promise.resolve(J({ ok: true, result: SESSIONS.filter(function (x) { return CLOSED.indexOf(x.id) < 0; }) }));
    }
    // DIAGNOSTIC BUILD (round 156): the SSE branch that broke the boot in round 154, back in place to have
    // its page errors read rather than guessed at.
    if (u.indexOf('/api/events/term') >= 0) {
      // THE ACTIVITY FIXTURE IS GONE (round 194). It pushed one playwright-changed control frame, and its
      // stated reason for existing was that it would "light something the day that pane is measurable". That
      // day cannot come here: the frame has exactly one consumer, EmbeddedBrowserPane, which needs the
      // Electron shell control server, and round 193 established that reaching it would take an app-level
      // test seam or an Electron-side runner this repository does not have.
      //
      // NOTHING EVER PASSED ?activity=1 — no sweep surface used it — so it was a branch no run exercised,
      // kept for a consumer no run can render. What stays is the part the CONNECTED state needs: a stream
      // that opens, sends one empty frame so the app sees traffic, and stays open.
      //
      // The escaping notes stay because they are still true: this file has three escaping layers and one of
      // them cost two rounds in 154-156.
      // THE STREAM CLOSES AFTER ONE FRAME, and that is load-bearing: round 156 made the panel render
      // CONNECTED with this exact shape, and round 194 found that a stream which stays open and sends a
      // second frame at 900ms puts four "reconnecting" rows back into the report. This prune removes the
      // ?activity=1 branch and NOTHING ELSE — a fixture change that alters behaviour is not a prune.
      window.__sse.opened = true;
      var sseBody = new ReadableStream({
        start: function (c) {
          var enc = new TextEncoder();
          c.enqueue(enc.encode('data:\\n\\n'));
          c.close();
        },
      });
      return Promise.resolve(new Response(sseBody, { status: 200, headers: { 'content-type': 'text/event-stream' } }));
    }
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
  // AND NO BACKTICKS, which the header above asks for and nothing enforced: a backtick in this template
  // ends the literal and the file stops parsing. Round 154 wrote two of them into a comment ABOUT the
  // other trap. The check is one line and would have caught all of them.
  if (stub.includes("`")) throw new Error("the stub contains a backtick — it would end the emitted template early");
  // AND IT MUST PARSE, which the backtick search above cannot tell you. Round 156 found why: an escape that
  // a three-layer edit (shell, python, template) collapsed to a single backslash became a REAL NEWLINE
  // inside a single-quoted string, the emitted stub was a syntax error, every request failed, and the app
  // sat on the connect screen while the emitter reported success. One compile names it immediately.
  try {
    new Function(stub);
  } catch (e) {
    throw new Error("the emitted stub does not parse: " + e.message);
  }
  // A STALE HARNESS MUST SAY SO. The panel sweep measures a DELIVERED copy of this file, and rounds 189
  // found one that predated round 172's tab-strip fix: its inlined CSS collapsed the strip to 17px, the
  // sweep reported real-looking overflow findings, and a waiver hid them. Nothing in a report said which
  // generation had been measured. This stamp is that missing fact — the built stylesheet's size and hash,
  // carried into every report so a reader can see the harness is older than the build it should match.
  const stamp = (() => {
    try {
      const css = readFileSync(join(ROOT, "agent", "resources", "panel", "panel.css"));
      return css.length + "-" + createHash("sha256").update(css).digest("hex").slice(0, 12);
    } catch (e) { return "unknown"; }
  })();
  return `<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="vale-harness-build" content="${stamp}">
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
