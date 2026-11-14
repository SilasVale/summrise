// TWO SLOTS, ONE WINDOW — THE TEST THAT COULD NOT EXIST BEFORE THE UNIFICATION.
//
// WHY THIS FILE EXISTS. Until `src/embedded-slot.ts`, the shell had two modules and each held ITS state at
// MODULE SCOPE: `embedded-view.ts` owned `embeddedView`/`embeddedVisible`/`embeddedBounds`/`embeddedUrl`,
// and `dsh-view.ts` owned `dshView`/`dshVisible`/`dshBounds`. A second instance of either was not merely
// unused, it was unrepresentable — so nothing had ever checked the property that a FACTORY has to get
// right: `place`/`recover`/`gone` on one slot must not touch the other's view, flags, bounds or channel.
// Every test in the two instance suites builds exactly ONE slot, so a merge that left a binding at module
// scope would pass all 38 of them and fail here. That is the defect this file is for.
//
// WHAT IT LOADS, AND HOW. The committed `src/embedded-slot.js`, evaluated against a stubbed Electron the
// same way `test/embedded-view.test.mjs` and `test/dsh-slot.test.mjs` do (a CommonJS module given its four
// globals, the bare `electron` specifier intercepted, every other specifier resolved by the real loader
// from `src/`), with the real policy wasm shared by all three files. Every expected VALUE is asked of the
// crates rather than restated, and every case opens with a `guard:` where the crate's answer is what makes
// the case interesting.
//
// ONE STUB, TWO SLOTS, AND THAT IS THE WHOLE SETUP. Both instances are built over the SAME window object —
// the same `contentView`, the same `webContents.send` — because that is the real deployment: two native
// views over the placeholders of one SPA window. What must NOT be shared is the slot state, so each case
// drives one slot and then asserts the other's view, record and channel are exactly where they were.
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SRC = path.join(HERE, "..", "src");
const SLOT_JS = path.join(SRC, "embedded-slot.js");

const require = createRequire(import.meta.url);
// THE TWO POLICY CRATES, as the shell ships them — the source of every expected value below.
const policy = require(path.join(SRC, "summrise_shell_policy.js"));
const urlPolicy = require(path.join(SRC, "summrise_url_policy.js"));

/** A port that is NOT the crate's default, so a slot that hard-coded the harness's address would fail. */
const PORT = 18997;

/** The Electron surface the module touches, for ONE window: `views` is every view built (in creation
 *  order, so a case can tell which slot built which), `attached` models a real `contentView` where
 *  re-adding MOVES a view rather than stacking a copy, and `sent` is the ONE channel out to the renderer —
 *  shared by both slots, which is exactly why a cross-instance push would be visible here. */
function fakeShell() {
  const views = [];
  const attached = [];
  const sent = [];
  const permissions = { request: null, check: null };

  class WebContents {
    constructor() {
      this.destroyed = false;
      this.url = "about:blank";
      this.title = "";
      this.loaded = [];
      this.reloaded = 0;
      this.closed = 0;
      this.history = { back: false, fwd: false };
      this.handlers = new Map();
      this.openHandler = null;
    }
    isDestroyed() { return this.destroyed; }
    getURL() { return this.url; }
    getTitle() { return this.title; }
    setWindowOpenHandler(handler) { this.openHandler = handler; }
    on(event, handler) {
      const list = this.handlers.get(event) ?? [];
      list.push(handler);
      this.handlers.set(event, list);
    }
    emit(event, ...args) { for (const handler of this.handlers.get(event) ?? []) handler(...args); }
    loadURL(url) { this.loaded.push(url); this.url = url; return Promise.resolve(); }
    close() { this.closed += 1; this.destroyed = true; }
    reload() { this.reloaded += 1; }
    setZoomFactor() { /* recorded by the instance suites; not a property of this file */ }
    get navigationHistory() {
      return { canGoBack: () => this.history.back, canGoForward: () => this.history.fwd };
    }
  }

  class WebContentsView {
    constructor(options) { this.options = options; this.webContents = new WebContents(); this.visible = null; this.bounds = null; views.push(this); }
    setVisible(visible) { this.visible = visible; }
    setBounds(bounds) { this.bounds = bounds; }
  }

  const win = {
    destroyed: false,
    isDestroyed() { return this.destroyed; },
    contentView: {
      addChildView(view) {
        const at = attached.indexOf(view);
        if (at >= 0) attached.splice(at, 1);
        attached.push(view);
      },
      removeChildView(view) {
        const at = attached.indexOf(view);
        if (at >= 0) attached.splice(at, 1);
      },
    },
    webContents: { send: (channel, payload) => sent.push([channel, payload]) },
  };

  const electron = {
    WebContentsView,
    BrowserWindow: class {},
    session: {
      defaultSession: {
        setPermissionRequestHandler(handler) { permissions.request = handler; },
        setPermissionCheckHandler(handler) { permissions.check = handler; },
      },
    },
  };

  return { electron, win, views, attached, sent, permissions };
}

/** The committed module, evaluated against `electron`. */
function loadSlotModule(electron) {
  const source = readFileSync(SLOT_JS, "utf8");
  const evaluated = { exports: {} };
  const resolvedFromSrc = createRequire(SLOT_JS);
  const requireForModule = (specifier) => (specifier === "electron" ? electron : resolvedFromSrc(specifier));
  new Function("exports", "require", "module", "__filename", "__dirname", source)(
    evaluated.exports,
    requireForModule,
    evaluated,
    SLOT_JS,
    SRC,
  );
  return evaluated.exports;
}

// ── THE TWO CONFIGS, EXACTLY AS `main.ts` BUILDS THEM ─────────────────────────────────────────────────
// Each carries its own door (with its own `asked` list, so a case can prove a slot asked ITS door and not
// the other's), its own popup policy, recovery target, state-url rule and channel names. Nothing is shared
// but the window — which is the point of the file.

/** The BROWSER's config: the ONE load door is the url policy's `sanitizeBrowserUrl`; the popup goes
 *  through it; recovery re-loads the last decided url; the record falls back to it; and the slot has the
 *  nav channel that brings the history surface. */
function browserConfig(shell, asked, overrides = {}) {
  const door = (raw) => { asked.push(raw); return urlPolicy.sanitizeBrowserUrl(raw); };
  return {
    win: () => shell.win,
    door,
    popupTarget: (raw) => policy.embeddedPopupTarget(door(raw)),
    recoverUrl: (lastDecided) => policy.embeddedRecoverUrl(lastDecided),
    stateUrl: (live, lastDecided) => policy.shownUrl(live, lastDecided),
    goneChannel: "embedded-browser:gone",
    navChannel: "embedded-browser:nav",
    ...overrides,
  };
}

/** The DSH's config: its own door (the shell's `dshTarget` over the url policy's `isDshUrl`), the DSH's
 *  popup decision, its own recovery target (always the harness home), a record that describes the live
 *  view only, and NO nav channel. */
function dshConfig(shell, asked, overrides = {}) {
  urlPolicy.setDshPort(PORT);
  return {
    win: () => shell.win,
    door: (raw) => { asked.push(raw); return policy.dshTarget(raw, urlPolicy.isDshUrl(raw)); },
    popupTarget: (raw) => policy.dshPopupTarget(raw, urlPolicy.isDshUrl(raw)),
    recoverUrl: () => policy.dshHome(urlPolicy.dshBase()),
    stateUrl: (live) => policy.shownUrl(live, ""),
    goneChannel: "embedded-dsh:gone",
    ...overrides,
  };
}

/** ONE window, ONE module evaluation, TWO slots — the pairing every case below drives. */
function twoSlots(buildB = dshConfig) {
  const shell = fakeShell();
  const mod = loadSlotModule(shell.electron);
  const askedA = [];
  const askedB = [];
  const slotA = mod.embeddedSlot(browserConfig(shell, askedA));
  const slotB = mod.embeddedSlot(buildB(shell, askedB));
  return Object.assign(shell, { mod, slotA, slotB, askedA, askedB });
}

test("two slots over ONE window place independently — one slot's place() moves nothing of the other's", () => {
  const { slotA, slotB, views, attached } = twoSlots();
  const boundsA = { x: 0, y: 0, width: 900, height: 500 };
  const boundsB = { x: 40, y: 60, width: 700, height: 400 };
  slotA.navigate("https://example.com/");
  slotB.navigate(`http://127.0.0.1:${PORT}/`);
  assert.equal(views.length, 2, "two slots are two views, and both attach to the ONE window's contentView");
  const [viewA, viewB] = views;
  assert.deepEqual(attached, [viewA, viewB], "both views share the window — placing one must not evict the other");
  assert.equal(viewA.visible, false, "both are built hidden: neither slot shows a view nobody placed");
  assert.equal(viewB.visible, false);
  // ── PLACE A ──────────────────────────────────────────────────────────────────────────────────────
  slotA.place(boundsA);
  assert.equal(viewA.visible, true);
  assert.deepEqual(viewA.bounds, boundsA, "A's bounds are A's");
  assert.equal(viewB.visible, false, "B was never placed, and A's place() must not show it");
  assert.equal(viewB.bounds, null, "nor give it bounds");
  assert.equal(slotA.state().visible, true);
  assert.equal(slotB.state().visible, false, "B's record must not claim A's placement");
  // ── PLACE B ──────────────────────────────────────────────────────────────────────────────────────
  slotB.place(boundsB);
  assert.equal(viewA.visible, true, "B's place() must not hide A");
  assert.deepEqual(viewA.bounds, boundsA, "and must not move it");
  assert.equal(viewB.visible, true);
  assert.deepEqual(viewB.bounds, boundsB, "each slot keeps its OWN bounds");
  assert.deepEqual(attached, [viewA, viewB], "and both stay children of the one window");
  // ── HIDE A ───────────────────────────────────────────────────────────────────────────────────────
  slotA.place(null);
  assert.equal(viewA.visible, false, "the SPA cleared A's placeholder");
  assert.equal(viewB.visible, true, "and B — whose placeholder is still there — is untouched");
  assert.deepEqual(viewB.bounds, boundsB);
  assert.equal(slotA.state().visible, false);
  assert.equal(slotB.state().visible, true);
});

test("two slots of the SAME shape are still two slots — the last decided url and the record are per instance", () => {
  // THE SHARPEST FORM OF THE PROPERTY: the same config, twice, so every difference below comes from the
  // INSTANCE and not from the config. A binding left at module scope would give both slots one record and
  // one last-decided url, and this case is what notices.
  const { slotA, slotB, views } = twoSlots(browserConfig);
  const urlA = "https://a.example/one";
  const urlB = "https://b.example/two";
  const decidedA = urlPolicy.sanitizeBrowserUrl(urlA);
  const decidedB = urlPolicy.sanitizeBrowserUrl(urlB);
  slotA.navigate(urlA);
  slotB.navigate(urlB);
  const [viewA, viewB] = views;
  assert.deepEqual(viewA.webContents.loaded, [decidedA]);
  assert.deepEqual(viewB.webContents.loaded, [decidedB]);
  assert.equal(slotA.state().url, decidedA, "each record reports ITS view's url");
  assert.equal(slotB.state().url, decidedB);
  // THE LAST DECIDED URL IS STATE TOO, and recovery is where it shows: A's fresh view must be reloaded at
  // A's last decided url, never at B's.
  viewA.webContents.destroyed = true;
  assert.deepEqual(slotA.recover(), { ok: true });
  assert.equal(views.length, 3, "A's recovery built A's own replacement");
  assert.deepEqual(views[2].webContents.loaded, [policy.embeddedRecoverUrl(decidedA)], "A recovers to A's last decided url");
  assert.equal(viewB.webContents.destroyed, false, "and B was not re-created");
  assert.deepEqual(viewB.webContents.loaded, [decidedB], "nor reloaded");
  assert.equal(slotB.state().url, decidedB, "nor did B's record change");
  // ...AND THE FLAGS ARE PER INSTANCE TOO: A was never placed, so A's recovery re-shows nothing, while B's
  // placement stays B's (B was placed after A recovered, at a spot A must not inherit).
  assert.equal(views[2].visible, false, "A's fresh view is hidden: A's slot was never placed");
  const boundsB = { x: 5, y: 5, width: 500, height: 500 };
  slotB.place(boundsB);
  assert.deepEqual(viewB.bounds, boundsB);
  assert.equal(views[2].bounds, null, "A's fresh view has no bounds of B's");
  assert.equal(slotA.state().visible, false, "and A's record says hidden");
  assert.equal(slotB.state().visible, true);
});

test("recover() on one slot leaves the other's view, flags and bounds exactly where they were", () => {
  const { slotA, slotB, views, attached } = twoSlots();
  const boundsA = { x: 1, y: 2, width: 800, height: 600 };
  const boundsB = { x: 3, y: 4, width: 640, height: 480 };
  slotA.navigate("https://example.com/");
  slotB.navigate(`http://127.0.0.1:${PORT}/`);
  slotA.place(boundsA);
  slotB.place(boundsB);
  const [viewA, viewB] = views;
  // A's renderer dies; B's does not.
  viewA.webContents.destroyed = true;
  assert.deepEqual(slotA.recover(), { ok: true });
  assert.equal(views.length, 3, "A's recovery built a NEW view");
  const freshA = views[2];
  assert.notEqual(freshA, viewA);
  assert.equal(viewB.webContents.destroyed, false, "B's webContents was never closed");
  assert.equal(viewB.webContents.closed, 0, "and never asked to close");
  assert.equal(viewB.visible, true, "B is still showing");
  assert.deepEqual(viewB.bounds, boundsB, "at B's own bounds");
  assert.deepEqual(attached, [viewB, freshA], "the window holds B and A's replacement — and A's is LAST, because attaching RAISES a view (which is how place() keeps one above the SPA content) and recovery re-attaches the fresh one");
  assert.deepEqual(freshA.bounds, boundsA, "and A's replacement took A's placement, not B's");
  assert.equal(freshA.visible, true);
  assert.equal(slotB.state().visible, true, "B's record is undisturbed");
  assert.equal(slotB.state().url, policy.shownUrl(`http://127.0.0.1:${PORT}/`, ""), "including the url B loaded");
  assert.equal(slotA.state().visible, true, "while A's record follows A's own recovery");
});

test("a crash on one slot pushes that slot's channel once, hides only its own view, and leaves the other slot's pushes working", () => {
  const { slotA, slotB, views, sent } = twoSlots();
  slotA.navigate("https://example.com/");
  slotB.navigate(`http://127.0.0.1:${PORT}/`);
  slotA.place({ x: 0, y: 0, width: 800, height: 600 });
  slotB.place({ x: 0, y: 0, width: 800, height: 600 });
  const [viewA, viewB] = views;
  sent.length = 0;   // the two navigations pushed their own nav events; start from a clean record
  // ── A CRASHES ────────────────────────────────────────────────────────────────────────────────────
  viewA.webContents.emit("render-process-gone", null, { reason: "crashed", exitCode: 111 });
  assert.deepEqual(sent, [["embedded-browser:gone", { reason: "crashed", exitCode: 111 }]],
    "exactly ONE push, on A's channel, carrying A's exit code — a slot that wrote shared state would push twice or on the wrong channel");
  assert.equal(viewA.visible, false);
  assert.equal(viewB.visible, true, "B's view is not the one that died");
  assert.equal(slotB.state().visible, true);
  // ── B'S CHANNELS STILL WORK ──────────────────────────────────────────────────────────────────────
  viewB.webContents.title = "Harness";
  viewB.webContents.history = { back: true, fwd: false };
  viewB.webContents.emit("did-navigate");
  assert.deepEqual(sent, [["embedded-browser:gone", { reason: "crashed", exitCode: 111 }]],
    "B's navigation pushed NOTHING — the DSH's slot has no nav channel, and A's crash did not give it one");
  // ── B CRASHES TOO ────────────────────────────────────────────────────────────────────────────────
  viewB.webContents.emit("render-process-gone", null, { reason: "oom", exitCode: 222 });
  assert.deepEqual(sent.at(-1), ["embedded-dsh:gone", { reason: "oom", exitCode: 222 }],
    "the DSH's crash pushes the DSH's channel — the two gone channels are configuration");
  assert.equal(sent.length, 2, "one push per crash, and no more");
  assert.equal(viewA.visible, false, "and A stays hidden: B's crash must not re-show A's dead view");
});

test("each slot opens only its own door — the two doors decide differently and neither touches the other's slot", () => {
  const { slotA, slotB, askedA, askedB, views } = twoSlots();
  // THE SAME RAW ADDRESS, TWO DOORS: the browser's admits any http/https, the DSH's admits only its own
  // loopback origin. Merging the doors — the difference the crate split forces to stay apart — would show
  // up here as one of these two lines changing.
  const stranger = "https://example.com/";
  assert.equal(urlPolicy.sanitizeBrowserUrl(stranger), stranger, "guard: the browser's door admits an https page");
  assert.equal(urlPolicy.isDshUrl(stranger), false, "guard: the url policy refuses it for the DSH");
  slotA.navigate(stranger);
  slotB.navigate(stranger);
  assert.deepEqual(askedA, [stranger], "A asked A's door, with the raw string");
  assert.deepEqual(askedB, [stranger], "B asked B's door, with the raw string — the doors are not shared");
  assert.deepEqual(views[0].webContents.loaded, [stranger], "the browser's slot went where its door said");
  assert.deepEqual(views[1].webContents.loaded, ["about:blank"], "the DSH's slot refused the same address");
  assert.equal(views[1].webContents.url, "about:blank", "and did not blank anything else: it never had a page");
  // ...AND A REFUSED POPUP IS PER SLOT TOO: the browser drops a `javascript:` link, the DSH drops the
  // stranger — neither ends up navigating the other's view.
  views[1].webContents.title = "Harness";
  assert.deepEqual(views[0].webContents.openHandler({ url: "javascript:alert(1)" }), { action: "deny" });
  assert.deepEqual(views[1].webContents.openHandler({ url: stranger }), { action: "deny" });
  assert.deepEqual(views[0].webContents.loaded, [stranger], "A's view did not move");
  assert.deepEqual(views[1].webContents.loaded, ["about:blank"], "and neither did B's");
});
