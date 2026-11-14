// THE BROWSER-SESSION WINDOWS, EXECUTED — the committed module, a stubbed Electron, the real policy wasm.
//
// WHY THIS FILE EXISTS. `src/browser-sessions.ts` is the shell's only module that creates the
// AI-drivable `BrowserWindow`s — the pages playwright reaches over the shared CDP endpoint — and its
// three operations have three call sites each outside it (the IPC door, the loopback control server and
// the panel's `target=_blank` handler). Nothing pinned what they DO. `test/ipc-door.test.mjs` can only
// COUNT things in the source, which is the right instrument for "one door" and the wrong one for "does a
// replay of the same URL stack a second window": a count over the text cannot answer it. So this file
// loads the module and runs it.
//
// WHAT IT LOADS, AND WHY THAT FILE. The committed `src/browser-sessions.js` — the CommonJS output that
// `src/main.js` requires and the release ships — not the `.ts`. CI already holds the pair together (the
// `electron src freshness` step re-runs `tsc` and `cmp`s the result against the committed `.js`), and the
// artifact worth executing is the one that reaches the operator's device.
//
// HOW IT LOADS IT WITHOUT INSTALLING ELECTRON. A CommonJS module needs four things — `exports`,
// `require`, `module`, `__filename`/`__dirname` — so this file supplies them and evaluates the source with
// its own `require`: the ONE specifier that has to be intercepted is the bare `electron`, and everything
// else (`./summrise_shell_policy`) goes through the real loader rooted at `src/`, so the module and this
// test share ONE instance of the policy wasm. The module's text is never edited, regexed or shimmed.
//
// A FRESH MODULE PER TEST. The module binds its host once and keeps the session table in module state, so
// a shared instance would make every test depend on the order the others ran in — and the sessions a test
// could not reach would be the ones hiding a defect. Each test therefore gets a new stub AND a new
// evaluation of the source, the move `gateway/wasm/run-cases.mjs` already makes and names at line 173:
// "a fresh instance is also what a new isolate is."
//
// WHAT IT DECIDES: NOTHING, AND THAT IS THE POINT. This file pins the module's WIRING — which platform
// call, with which argument, at which moment — and every expected VALUE is read back out of the policy
// crate the shell ships (`summrise_shell_policy.js`): the reuse/evict plan, the window id, the CDP
// endpoint and the cap are ASKED of the crate rather than restated here, so a policy change cannot leave
// this file asserting yesterday's answer. Each case that turns on a policy answer opens with a `guard:`
// assertion that the crate really says what makes the case interesting, so a case cannot pass by
// measuring nothing.
//
// WHAT IT CANNOT SEE, and no test here can: a real Chromium window, the CDP :9333 endpoint a playwright
// client actually reaches, whether an icon file is a decodable image (`main.ts`'s `windowIcon` resolves
// the path; `probeImage` is what asks `nativeImage`), and real `beforeunload` behaviour — the whole
// reason `browserClose` uses `destroy()` rather than `close()` is a fact about Chromium, and what is
// pinned below is only that `destroy()` is the call the module makes. This file's stub is the platform,
// and the only claim it makes about Electron is the shape of the calls the module makes on it.
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SRC = path.join(HERE, "..", "src");
const SESSIONS_JS = path.join(SRC, "browser-sessions.js");

const require = createRequire(import.meta.url);
// THE TWO POLICY CRATES, as the shell ships them — the source of every expected value below.
const policy = require(path.join(SRC, "summrise_shell_policy.js"));
const urlPolicy = require(path.join(SRC, "summrise_url_policy.js"));
const CAP = JSON.parse(policy.shellConstants()).MAX_BROWSER_WINDOWS;
const CDP_PORT = JSON.parse(policy.shellConstants()).CDP_PORT;
/** The id NAMESPACE the crate mints in — DERIVED from a real id rather than restated, so this file cannot
 *  go on asserting yesterday's format: everything up to and including the first `-` of `browserId(…)`.
 *  `browser-<ms>` and `browser-<ms>-<n>` share it, which is why a counter appended after the millisecond
 *  moved no case that only asks for the namespace. */
const ID_NAMESPACE = (() => {
  const sample = policy.browserId(0);
  return sample.slice(0, sample.indexOf("-") + 1);
})();

/** The Electron surface `browser-sessions.js` touches, and NOTHING besides it: every method below exists
 *  because the module calls it, so this object is that module's platform boundary read off the module
 *  rather than invented beside it. `windows` is every window the module built, in creation order —
 *  `BrowserWindow` is not a singleton in Electron, so the stub records instances instead of a last one. */
function fakeElectron() {
  const windows = [];

  class WebContents {
    constructor(owner) {
      this.owner = owner;
      this.destroyed = false;
      this.url = "about:blank";
      this.loaded = [];
      this.handlers = new Map();
      this.openHandler = null;
    }
    isDestroyed() { return this.destroyed; }
    getURL() { return this.url; }
    setWindowOpenHandler(handler) { this.openHandler = handler; }
    on(event, handler) {
      const list = this.handlers.get(event) ?? [];
      list.push(handler);
      this.handlers.set(event, list);
    }
    /** What Electron does when the event fires: the module's own listeners, in order. */
    emit(event, ...args) { for (const handler of this.handlers.get(event) ?? []) handler(...args); }
    loadURL(url) { this.loaded.push(url); this.url = url; return Promise.resolve(); }
  }

  class BrowserWindow {
    constructor(options) {
      this.options = options;
      this.webContents = new WebContents(this);
      this.destroyed = false;
      this.titles = [];
      this.shown = 0;
      this.focused = 0;
      this.restored = 0;
      this.minimized = false;
      this.closeCalls = 0;
      this.handlers = new Map();
      windows.push(this);
    }
    isDestroyed() { return this.destroyed; }
    isMinimized() { return this.minimized; }
    restore() { this.restored += 1; this.minimized = false; }
    show() { this.shown += 1; }
    focus() { this.focused += 1; }
    setTitle(title) { this.titles.push(title); }
    on(event, handler) {
      const list = this.handlers.get(event) ?? [];
      list.push(handler);
      this.handlers.set(event, list);
    }
    /** What Electron does when the event fires: the module's own listeners, in order. */
    emit(event, ...args) { for (const handler of this.handlers.get(event) ?? []) handler(...args); }
    /** `BrowserWindow.loadURL()` is a real Electron convenience that forwards to the webContents, and
     *  it is the call the module makes — the stub forwards it the same way so the assertions can read
     *  one place (`webContents.loaded`) for what was loaded. */
    loadURL(url) { return this.webContents.loadURL(url); }
    /** `destroy()` is what the module uses, and it is NOT `close()`: a page's beforeunload can veto
     *  `close()`, which is the defect `review #g` records. The stub keeps them apart so a rewrite that
     *  reached for `close()` would fail here instead of hanging on a device. */
    destroy() { this.destroyed = true; }
    close() { this.closeCalls += 1; this.destroyed = true; }
  }

  return { electron: { BrowserWindow }, windows };
}

/** The committed module, evaluated against `electron` — a CommonJS module with its one bare specifier
 *  answered by the stub and every other specifier resolved by the real loader, from `src/`. */
function loadBrowserSessions(electron) {
  const source = readFileSync(SESSIONS_JS, "utf8");
  const evaluated = { exports: {} };
  const resolvedFromSrc = createRequire(SESSIONS_JS);
  const requireForModule = (specifier) => (specifier === "electron" ? electron : resolvedFromSrc(specifier));
  new Function("exports", "require", "module", "__filename", "__dirname", source)(
    evaluated.exports,
    requireForModule,
    evaluated,
    SESSIONS_JS,
    SRC,
  );
  return evaluated.exports;
}

/** A bound module over a fresh stub, with the seam `main.ts` really binds: `loadTarget` IS the URL
 *  policy's `sanitizeBrowserUrl`, wrapped so the test can see the RAW string the module handed it, and
 *  `windowIcon` is a stub this test chooses per case.
 *
 *  `Date.now` IS STUBBED FOR THE DURATION OF EVERY CALL, AND THAT IS NOT CONVENIENCE. The module stamps
 *  each window's id with `browserId(Date.now())`, and the crate answers `browser-<ms>-<n>` — the
 *  millisecond PLUS a counter, so two opens inside ONE millisecond are two ids and the session table
 *  tracks windows. WHEN IT DID NOT, this file's first draft measured a table of ONE from eight opens in a
 *  tight loop and proved nothing about the cap: the id was the bare millisecond, the second open
 *  overwrote the first, and the window it overwrote became unreachable. The clock stays stubbed because
 *  the id still CARRIES the millisecond the module handed the crate, and a case that checks that field
 *  needs to know what `Date.now()` answered. The patch is restored in a `finally`, so no other test in
 *  the run can observe it.
 *
 *  THE CLOCK IS A MODE, AND NO MODE LEAVES THE REAL CLOCK IN PLACE — that absence is the point.
 *  `"step"` (the default) hands a fresh millisecond to each open, which is what a real operator's opens
 *  do. `"frozen"` hands the SAME millisecond to every open in the case, which is what makes the two id
 *  cases interesting: they assert that a SHARED millisecond still yields distinct, separately reachable
 *  ids — the counter doing the work and not the clock. The mode this replaced was a boolean whose `false`
 *  ran on the real clock, and the collision case took it — so whether two opens shared a millisecond was
 *  decided by how long `browserOpen` took to run. That held on a warm local process and failed in CI
 *  (`browser-…588` against `…589`, a real millisecond apart). A case whose subject is a shared
 *  millisecond has to MAKE one; the real clock is not an instrument. An unknown mode throws here rather
 *  than falling through to it. */
function boundModule({ icon = "", clock = "step" } = {}) {
  const shell = fakeElectron();
  const mod = loadBrowserSessions(shell.electron);
  const asked = [];
  mod.browserSessionsBind({
    loadTarget: (raw) => { asked.push(raw); return urlPolicy.sanitizeBrowserUrl(raw); },
    windowIcon: () => icon,
    maxBrowserWindows: CAP,
  });
  if (clock !== "step" && clock !== "frozen") {
    throw new Error(`unknown clock mode ${JSON.stringify(clock)} — "step" and "frozen" are the only two, and neither leaves the real clock in place`);
  }
  const real = Date.now;
  // Read ONCE from the real clock, so a frozen id still looks like one a device minted rather than a
  // number this file invented. It is the value the `"frozen"` case asserts against, which is what makes
  // that case's guard a claim about the module rather than about its own arithmetic.
  const FROZEN_MS = real.call(Date);
  let tick = 0;
  // STRICTLY INCREASING, and seeded from the real clock so the id still looks like one a device would
  // mint. A counter that merely added 1 to its own previous value could not move forward twice inside
  // one real millisecond, which is exactly the collision this mode exists to step around.
  const stamp = clock === "frozen"
    ? () => FROZEN_MS
    : () => { tick = Math.max(tick + 1, real.call(Date) + 1); return tick; };
  // ONE MILLISECOND PER OPEN, REMEMBERED. A real `browserOpen` reads `Date.now()` once, and the id cases
  // need to know WHICH millisecond the module handed the crate: `stampedAt()` answers that, and fixing
  // the value for the duration of the call is what keeps it the same answer if the module ever read the
  // clock twice. It is the frozen millisecond in `"frozen"` mode, so a guard can prove the two opens of a
  // collision case really do share one.
  let lastStamp = null;
  const open = (url) => {
    let fixed = null;
    Date.now = () => (fixed ??= stamp());
    try { return mod.browserOpen(url); } finally { lastStamp = fixed; Date.now = real; }
  };
  return { ...shell, mod, asked, open, FROZEN_MS, stampedAt: () => lastStamp };
}

test("an unbound module throws from the operation that needs the host, and still answers reads of an empty table", () => {
  const { electron } = fakeElectron();
  const mod = loadBrowserSessions(electron);
  const LOUD = /browserSessionsBind\(\) was never called/;
  // `browserOpen` reads the load door FIRST, which is what makes an unbound call fail loudly instead of
  // opening a window through some other load path. It is the ONLY operation that needs the host: the
  // other two read the session table and the module's own constant, so an unbound shell can still close
  // nothing and list nothing. Asserting the throw on all three would have been an assertion about the
  // test rather than the module.
  assert.throws(() => mod.browserOpen("https://example.com"), LOUD, "open must refuse before it can build a window");
  assert.deepEqual(mod.browserClose("browser-0"), { ok: true }, "closing a session on an empty table is not an error");
  assert.deepEqual(
    mod.browserList(),
    { ok: true, sessions: [], cdp: policy.cdpEndpoint(CDP_PORT) },
    "and listing one is an empty list with the crate's endpoint — no view and no host needed to answer",
  );
});

test("the module's five exports are exactly its surface — four main.ts calls by name, plus the wiring helper", () => {
  const { electron } = fakeElectron();
  const mod = loadBrowserSessions(electron);
  // The WHOLE surface, not only the names main.ts imports: a RENAMED export is a `TypeError` in a shell
  // that no suite here can start, and `browserCloseAll` in particular is called from exactly one place
  // (`before-quit`), which is a path no runtime test of this package can walk.
  assert.deepEqual(
    Object.keys(mod).sort(),
    ["browserClose", "browserCloseAll", "browserList", "browserOpen", "browserSessionsBind"],
    "a new export is a new boundary — main.ts, preload.ts and the panel's fixture have to be told about it",
  );
});

test("open() decides the RAW url through the ONE load door and loads the door's answer", () => {
  const { asked, open, windows } = boundModule();
  const raw = "https://example.com/x";
  const decided = urlPolicy.sanitizeBrowserUrl(raw);
  const answer = open(raw);
  assert.deepEqual(asked, [raw], "the door must be asked with the RAW string the AI handed over");
  assert.equal(windows.length, 1);
  assert.deepEqual(windows[0].webContents.loaded, [decided], "loadURL gets the door's answer, never the raw string");
  assert.equal(answer.url, decided, "and the answer reports what was decided");
});

test("open() on a REFUSED scheme opens the door's blank target, never the raw javascript: url", () => {
  const raw = "javascript:alert(1)";
  const decided = urlPolicy.sanitizeBrowserUrl(raw);
  assert.notEqual(decided, raw, "guard: the crate REFUSES this scheme (otherwise this case pins nothing)");
  const { open, windows } = boundModule();
  assert.equal(open(raw).url, decided);
  assert.deepEqual(windows[0].webContents.loaded, [decided], "a file:// or javascript: target must not reach loadURL as written");
});

test("open() on an existing target REUSES that window instead of stacking a duplicate", () => {
  const { mod, open, windows } = boundModule();
  const url = "https://example.com/";
  const first = open(url);
  // Guard: the crate's plan is what makes reuse possible, so ask IT rather than restating the rule.
  const plan = JSON.parse(policy.planBrowserOpen(
    JSON.stringify([{ id: first.id, target: url, destroyed: false }]), url, CAP,
  ));
  assert.equal(plan.reuse, first.id, "guard: the crate reuses the live window on the same target");
  const second = open(url);
  assert.equal(second.id, first.id, "a same-URL reopen must reuse the window, not stack a second one");
  assert.equal(windows.length, 1, "and no second BrowserWindow may be built");
  assert.equal(windows[0].shown, 1, "the reused window is SHOWN (it may be behind another)");
  assert.equal(windows[0].focused, 1, "and FOCUSED — the caller asked for this page now");
  assert.equal(mod.browserList().sessions.length, 1, "the table still holds one session, not two");
});

test("open() reuses on the window's INITIAL target, not on webContents.getURL()", () => {
  const { open, windows } = boundModule();
  const url = "https://example.com/";
  const first = open(url);
  // The defect this pins: `getURL()` lags during navigation (it reports about:blank while loading), so a
  // same-URL reopen right after the first open missed the reuse and stacked a duplicate. The stub makes
  // that lag explicit — the window has navigated AWAY, and the plan must still match on what it was
  // opened FOR.
  windows[0].webContents.url = "about:blank";
  const second = open(url);
  assert.equal(second.id, first.id, "reuse matches the stored INITIAL target, not the live getURL()");
  assert.equal(windows.length, 1);
});

test("open() at the cap evicts the OLDEST session, through the crate's plan and destroy()", () => {
  const { mod, open, windows } = boundModule();
  for (let i = 0; i < CAP; i++) assert.equal(open(`https://example.com/${i}`).ok, true);
  assert.equal(windows.length, CAP, "guard: the cap is the crate's number of windows, all live");
  assert.equal(mod.browserList().sessions.length, CAP, "guard: and the table holds every one of them");
  const oldest = windows[0];
  const extra = open("https://example.com/extra");
  assert.equal(extra.ok, true);
  assert.equal(windows.length, CAP + 1, "a new target at the cap opens one more window …");
  assert.equal(oldest.destroyed, true, "… and the EVICTED one is destroyed — the cap never grows");
  assert.equal(oldest.closeCalls, 0, "eviction is destroy(), never a close() a page's beforeunload can veto");
  assert.equal(mod.browserList().sessions.length, CAP, "the table reports the cap's count after the eviction");
});

test("open() on a target whose only window is DESTROYED reuses nothing and evicts nothing — it opens fresh", () => {
  const { mod, open, windows } = boundModule();
  const url = "https://example.com/";
  const first = open(url);
  windows[0].destroyed = true; // the OS closed it; the module's `closed` event has not run yet
  // Guard: the crate is what says a destroyed row is not reused — asked of the crate, not restated.
  const plan = JSON.parse(policy.planBrowserOpen(
    JSON.stringify([{ id: first.id, target: url, destroyed: true }]), url, CAP,
  ));
  assert.equal(plan.reuse, null, "guard: a destroyed window is not reused");
  assert.equal(plan.evict, null, "guard: and below the cap there is nothing to evict either");
  const second = open(url);
  assert.notEqual(second.id, first.id, "a dead window cannot be shown again — a fresh one is built");
  assert.equal(windows.length, 2, "the dead window is left for its `closed` event to clean up, not re-used");
  // The module's own listener is what empties the table: the stub fires the event Electron fires when a
  // window goes away, and the stale row must not stay behind to be listed as a live session.
  windows[1].destroyed = true;
  windows[0].emit("closed");
  windows[1].emit("closed");
  assert.deepEqual(mod.browserList().sessions, [], "a closed window must leave the session table");
});

test("a DESTROYED window still COUNTS against the cap — at the cap it is the eviction candidate", () => {
  const { mod, open, windows } = boundModule();
  for (let i = 0; i < CAP; i++) open(`https://example.com/${i}`);
  // Kill the oldest WITHOUT letting its `closed` listener run: this is the window between the OS
  // destroying a window and the module hearing about it, and the row is still in the table.
  const dead = mod.browserList().sessions[0];
  windows[0].destroyed = true;
  // Guard: ask the crate about the row the MODULE built — the defect this pins is about the ROW's shape
  // (its `destroyed` flag and its slot), so a hand-written row here would be the test grading itself.
  const plan = JSON.parse(policy.planBrowserOpen(
    JSON.stringify(mod.browserList().sessions.map((s) => ({ id: s.id, target: s.url, destroyed: s.id === dead.id }))),
    "https://example.com/extra",
    CAP,
  ));
  assert.equal(plan.evict, dead.id, "guard: the destroyed row is the crate's eviction candidate, so it still occupies a slot");
  const extra = open("https://example.com/extra");
  assert.equal(extra.ok, true);
  assert.notEqual(extra.id, dead.id, "the dead session's id must not come back");
  assert.equal(mod.browserList().sessions.length, CAP, "the dead row is evicted and the live count is back at the cap");
});

test("close() destroys the window and forgets it, and is a no-op for an id it never had", () => {
  const { mod, open, windows } = boundModule();
  const { id } = open("https://example.com/");
  assert.deepEqual(mod.browserClose(id), { ok: true });
  assert.equal(windows[0].destroyed, true, "close() destroys — review #g: close() is vetoable, destroy() is not");
  assert.equal(windows[0].closeCalls, 0);
  assert.deepEqual(mod.browserList().sessions, [], "the destroyed session must leave the table");
  // An unknown id is not an error: the SPA and the loopback control server both call this on a guess.
  assert.deepEqual(mod.browserClose("browser-999"), { ok: true });
  assert.deepEqual(mod.browserClose(), { ok: true }, "and no id at all answers the same way (the control route sends one)");
});

test("list() reports the LIVE url per session, so one that navigated away is listed where it is now", () => {
  const { mod, open, windows } = boundModule();
  const opened = open("https://example.com/");
  windows[0].webContents.url = "https://example.com/inner";
  // The asymmetry with REUSE is deliberate and is the crate's: reuse needs the target the window was
  // OPENED for (getURL() lags), while the list is a report of where each session IS.
  assert.deepEqual(mod.browserList().sessions, [{ id: opened.id, url: "https://example.com/inner" }]);
});

test("list() answers the crate's CDP endpoint, so a client can attach to the sessions it names", () => {
  const { mod } = boundModule();
  assert.equal(policy.cdpEndpoint(CDP_PORT), "http://127.0.0.1:9333", "guard: the crate's endpoint for the shell's port");
  assert.equal(mod.browserList().cdp, policy.cdpEndpoint(CDP_PORT), "the endpoint is the crate's, not a literal here");
  assert.equal(mod.browserOpen("https://example.com/").cdp, policy.cdpEndpoint(CDP_PORT), "and open() advertises the same one");
});

test("open() builds the window at the shell's size with the crate's id and the security posture", () => {
  const current = boundModule();
  const { open, windows } = current;
  const url = "https://example.com/x";
  const answer = open(url);
  const bw = windows[0];
  assert.equal(bw.options.width, 1100, "the session window is the shell's size, not Electron's default");
  assert.equal(bw.options.height, 750);
  assert.equal(bw.options.title, `Summrise Browser — ${answer.url}`, "the title carries the DECIDED target");
  // These windows load ARBITRARY internet pages: no preload key at all (zero Node/bridge surface),
  // context isolation on, node integration off, sandbox on.
  assert.deepEqual(
    bw.options.webPreferences,
    { contextIsolation: true, nodeIntegration: false, sandbox: true },
    "a session window renders untrusted pages — a preload here would hand a remote page the bridge",
  );
  // THE ID IS THE CRATE'S, asked of the crate rather than restated: `browserId` is what stamps it, and
  // what this case checks is the field the module SUPPLIES — the millisecond it read — prefixed by the
  // crate's namespace. What follows that millisecond is the crate's own business (a counter today, and
  // the reason two opens in one millisecond stay two windows); this file does not spell it.
  const stampedAt = `${ID_NAMESPACE}${current.stampedAt()}`;
  assert.equal(
    answer.id.slice(0, stampedAt.length),
    stampedAt,
    `the id must be the crate's, stamped with the millisecond the module read (${stampedAt}…) — a hand-built string is a second format`,
  );
});

test("open() DENIES popups — an unmanaged window would sit outside the cap", () => {
  const { open, windows } = boundModule();
  open("https://example.com/");
  // Guard: the handler exists and answers a refusal, so this is not an assertion about an absent key.
  assert.equal(typeof windows[0].webContents.openHandler, "function", "open() must install a window-open handler");
  assert.deepEqual(
    windows[0].webContents.openHandler({ url: "https://elsewhere.example/" }),
    { action: "deny" },
    "a popup from a session page is denied outright — browserOpen's own cap cannot see windows it did not create",
  );
});

test("open() follows the page's <title> so the taskbar shows the real page", () => {
  const { open, windows } = boundModule();
  open("https://example.com/");
  const bw = windows[0];
  bw.webContents.emit("page-title-updated", null, "Example Domain");
  assert.deepEqual(bw.titles, ["Summrise Browser — Example Domain"], "the site's own title replaces the target after load");
  // A destroyed window is sent nothing (setTitle on one throws in Electron) and an empty title is not a
  // title — both are guarded in the handler, and a rewrite that dropped either guard would crash here.
  bw.webContents.emit("page-title-updated", null, "");
  assert.equal(bw.titles.length, 1, "an empty title must not clear the window's title");
  bw.destroyed = true;
  bw.webContents.emit("page-title-updated", null, "Late");
  assert.equal(bw.titles.length, 1, "a destroyed window must be looked at before it is titled");
});

test("the window icon is asked from the host per window, and omitted when the host resolves none", () => {
  const withIcon = boundModule({ icon: "C:\\app\\icon.png" });
  withIcon.open("https://example.com/");
  assert.equal(withIcon.windows[0].options.icon, "C:\\app\\icon.png", "the shell's resolved icon path reaches BrowserWindow");
  const without = boundModule({ icon: "" });
  without.open("https://example.com/");
  assert.equal("icon" in without.windows[0].options, false, "no icon means the key is ABSENT, so Electron's default applies");
});

test("closeAll() destroys every live session and empties the table — the before-quit sweep", () => {
  const { mod, open, windows } = boundModule();
  open("https://example.com/a");
  open("https://example.com/b");
  assert.equal(windows.length, 2, "guard: two DISTINCT ids, so the sweep has two rows to walk");
  windows[1].destroyed = true; // one died on its own; the sweep must not destroy it a second time
  assert.equal(mod.browserCloseAll(), undefined, "the sweep answers nothing — main.ts's before-quit handler ignores it");
  assert.equal(windows[0].destroyed, true, "every live session window is destroyed on exit");
  assert.deepEqual(mod.browserList().sessions, [], "and the table is emptied, not merely walked");
  assert.equal(windows[1].closeCalls, 0, "the already-dead window is left alone rather than re-destroyed");
});

test("two opens inside ONE millisecond are two ids, and the sweep reaches BOTH windows", () => {
  // THE CASE THIS FILE USED TO PIN AS PRE-EXISTING DEFECT, RE-POINTED. Every assertion below is the same
  // measurement turned around: `browserId(Date.now())` used to answer `browser-<ms>`, so two opens in one
  // millisecond produced ONE id, the second `set()` overwrote the first, and the first window became
  // unreachable — nothing could reuse it, close it or sweep it. The crate now appends a monotonic
  // counter, so the millisecond is still the field a support log reads and the id still names ONE window.
  // The subject has not changed (a SHARED millisecond); only the required answer has.
  //
  // THE MILLISECOND IS FORCED, NOT HOPED FOR. This case used to bind with `clock: false`, i.e. the real
  // clock, and assert the two ids came out EQUAL — a claim about how fast two `browserOpen` calls run.
  // It held on this development box and failed in CI, where a millisecond ticked between the opens and
  // the ids came out `browser-…588` and `…589`. `"frozen"` makes the subject of the case true by
  // construction, and the `guard:` lines below prove the frozen millisecond stamped BOTH ids, so the
  // case still cannot pass by measuring nothing.
  const { mod, open, windows, FROZEN_MS, stampedAt } = boundModule({ clock: "frozen" });
  const first = open("https://example.com/a");
  assert.equal(stampedAt(), FROZEN_MS, "guard: the first open read the frozen millisecond");
  const second = open("https://example.com/b");
  assert.equal(stampedAt(), FROZEN_MS, "guard: and so did the second — the clock really did hold still");
  const frozen = `${ID_NAMESPACE}${FROZEN_MS}`;
  assert.equal(first.id.slice(0, frozen.length), frozen, "guard: the first id carries the frozen millisecond");
  assert.equal(second.id.slice(0, frozen.length), frozen, "guard: and so does the second — the millisecond ALONE is not an identifier");
  assert.notEqual(first.id, second.id, "the crate's counter is what separates two opens inside one millisecond");
  assert.equal(windows.length, 2, "two windows really were built, one per id");
  assert.deepEqual(
    mod.browserList().sessions.map((s) => s.id).sort(),
    [first.id, second.id].sort(),
    "and the table holds BOTH rows — neither window was overwritten",
  );
  mod.browserCloseAll();
  assert.equal(windows[0].destroyed, true, "the sweep reaches the first window — the orphan this case used to measure");
  assert.equal(windows[1].destroyed, true, "and the second");
  assert.deepEqual(mod.browserList().sessions, [], "and the table is emptied");
});

test("a same-millisecond burst is capped over WINDOWS, and the sweep reaches every one of them", () => {
  // THE MEASUREMENT THAT NAMED THE DEFECT, TURNED AROUND. With the cap at 8 and 11 distinct URLs under a
  // frozen clock, the bare millisecond id produced ONE table row: `existing.len() >= cap` was never true,
  // so nothing was evicted, 11 windows were built, and `browserCloseAll()` — which walks the TABLE —
  // destroyed 1 and left 10 ALIVE. The cap counts table rows and the resource is windows; the counter is
  // what makes those the same thing, so no separate repair of the cap or the sweep is owed.
  const { mod, open, windows } = boundModule({ clock: "frozen" });
  const BURST = CAP + 3;
  for (let i = 0; i < BURST; i++) {
    assert.equal(open(`https://example.com/${i}`).ok, true);
    const live = windows.filter((w) => !w.isDestroyed()).length;
    assert.equal(
      live,
      Math.min(i + 1, CAP),
      `open #${i + 1} of ${BURST} in ONE millisecond: the cap fills to ${CAP} and then holds — no open may add a window the table cannot name`,
    );
    assert.equal(
      mod.browserList().sessions.length,
      live,
      `open #${i + 1}: the table must hold one row per LIVE window (${live}) — a row the cap cannot see is the defect this pins`,
    );
  }
  assert.equal(windows.length, BURST, "guard: every open really built a window, so the burst is larger than the cap");
  mod.browserCloseAll();
  assert.deepEqual(
    windows.filter((w) => !w.isDestroyed()),
    [],
    "the before-quit sweep must reach EVERY window the burst built — these are the orphans that used to survive it",
  );
  assert.deepEqual(mod.browserList().sessions, [], "and the table is emptied");
});
