// THE EMBEDDED VIEW, EXECUTED — the committed module, a stubbed Electron, the real policy wasm.
//
// WHY THIS FILE EXISTS. `src/embedded-view.ts` is the shell's only module that creates a
// `WebContentsView`, and it was verified exactly once by a throwaway smoke harness under `/tmp` — outside
// this repository, so the verification was not repeatable by CI and nothing went red the day one of these
// behaviours changed. The two tests that were here before it read SOURCE AS TEXT for a good reason
// (`test/ipc-door.test.mjs`: `main.ts` imports electron, and this package deliberately installs nothing —
// its `npm test` is `node --test` over node builtins), but the properties that were unpinned are RUNTIME
// ones: is the view attached hidden, does a crash hide the dead one and push `gone`, does a zoom of 0 end
// at 100%? A count over the text cannot answer any of them. So this file loads the module and runs it.
//
// WHAT IT LOADS, AND WHY THAT FILE. The committed `src/embedded-view.js` — the CommonJS output that
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
// A FRESH MODULE AND A FRESH STUB PER TEST. The module binds its host once at startup and keeps its view,
// URL, bounds and visibility in module state, so a shared instance would make every test depend on the
// order the others ran in — and the state a test could not reach would be the one that hid a defect. Each
// test therefore gets a new stub AND a new evaluation of the source, the move `gateway/wasm/run-cases.mjs`
// already makes and names at line 173: "a fresh instance is also what a new isolate is."
//
// WHAT IT DECIDES: NOTHING, AND THAT IS THE POINT. This file pins the module's WIRING — which platform
// call, with which argument, at which moment — and every expected VALUE is read back out of the policy
// crates the shell ships (`summrise_shell_policy.js`, `summrise_url_policy.js`): the zoom clamp, the
// minimum slot, the popup drop rule and the recovery URL are asked of the crates rather than restated
// here, so a policy change cannot leave this file asserting yesterday's answer, and a value disagreement
// is the crates' business. Each case opens with a `guard:` assertion that the crate really answers what
// makes the case interesting, so a case cannot pass by measuring nothing.
//
// WHAT IT CANNOT SEE, and no test here can: real Chromium compositing, the CDP :9333 target the AI drives,
// the real `session.defaultSession` (its two permission hooks are recorded and CALLED — asserted to deny —
// but never against Chromium's implementation), and whether the committed `.wasm` LOADS — that last one is
// `test/shell-policy-wasm.test.mjs` and `test/url-policy-wasm.test.mjs`, which execute the artifacts
// directly. This file's stub is the platform, and the only claim it makes about Electron is the shape of
// the calls the module makes on it.
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SRC = path.join(HERE, "..", "src");
const VIEW_JS = path.join(SRC, "embedded-view.js");

const require = createRequire(import.meta.url);
// THE TWO POLICY CRATES, as the shell ships them — the source of every expected value below.
const policy = require(path.join(SRC, "summrise_shell_policy.js"));
const urlPolicy = require(path.join(SRC, "summrise_url_policy.js"));

/** The Electron surface `embedded-view.js` touches, and NOTHING besides it: every method below exists
 *  because the module calls it, so this object is that module's platform boundary read off the module
 *  rather than invented beside it. `attached` models a real `contentView`: re-adding a view MOVES it
 *  (it does not stack a copy), which is how the module keeps the view above the SPA's placeholder. */
function fakeShell({ withWindow = true } = {}) {
  const attached = [];
  const sent = [];
  const permissions = { request: null, check: null };

  class WebContents {
    constructor() {
      this.destroyed = false;
      this.url = "about:blank";
      this.title = "";
      this.loaded = [];
      this.zooms = [];
      this.went = [];
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
    /** What Electron does when the event fires: the module's own listeners, in order. */
    emit(event, ...args) { for (const handler of this.handlers.get(event) ?? []) handler(...args); }
    loadURL(url) { this.loaded.push(url); this.url = url; return Promise.resolve(); }
    close() { this.closed += 1; this.destroyed = true; }
    goBack() { this.went.push("back"); }
    goForward() { this.went.push("fwd"); }
    reload() { this.reloaded += 1; }
    setZoomFactor(factor) { this.zooms.push(factor); }
    get navigationHistory() {
      return { canGoBack: () => this.history.back, canGoForward: () => this.history.fwd };
    }
  }

  class WebContentsView {
    constructor(options) { this.options = options; this.webContents = new WebContents(); this.visible = null; this.bounds = null; }
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

  return { electron, win: withWindow ? win : null, attached, sent, permissions };
}

/** The committed module, evaluated against `electron` — a CommonJS module with its one bare specifier
 *  answered by the stub and every other specifier resolved by the real loader, from `src/`. */
function loadEmbeddedView(electron) {
  const source = readFileSync(VIEW_JS, "utf8");
  const evaluated = { exports: {} };
  const resolvedFromSrc = createRequire(VIEW_JS);
  const requireForModule = (specifier) => (specifier === "electron" ? electron : resolvedFromSrc(specifier));
  new Function("exports", "require", "module", "__filename", "__dirname", source)(
    evaluated.exports,
    requireForModule,
    evaluated,
    VIEW_JS,
    SRC,
  );
  return evaluated.exports;
}

/** A bound module over a fresh stub, with the door `main.ts` really binds: `loadTarget` IS the URL
 *  policy's `sanitizeBrowserUrl`, wrapped so the test can see the RAW string the module handed it. */
function boundModule(options) {
  const shell = fakeShell(options);
  const mod = loadEmbeddedView(shell.electron);
  const asked = [];
  mod.embeddedViewBind({
    win: () => shell.win,
    loadTarget: (raw) => { asked.push(raw); return urlPolicy.sanitizeBrowserUrl(raw); },
  });
  return { ...shell, mod, asked };
}

test("an unbound module throws the loud bind error from every operation that needs the host", () => {
  const { electron } = fakeShell();
  const mod = loadEmbeddedView(electron);
  const LOUD = /embeddedViewBind\(\) was never called/;
  // `embeddedNavigate` reads the door FIRST, which is what makes an unbound call fail loudly instead of
  // falling back to some other load path and quietly showing a blank page.
  assert.throws(() => mod.embeddedNavigate("https://example.com"), LOUD, "navigate must refuse before it can load anything");
  assert.throws(() => mod.embeddedViewEnsure(), LOUD, "ensure must refuse rather than attach a view to nothing");
  assert.throws(() => mod.embeddedViewPlace({ x: 0, y: 0, width: 800, height: 600 }), LOUD);
  assert.throws(() => mod.embeddedRecover(), LOUD);
});

test("the module's ten exports are functions — nine main.ts destructures by name, plus the wiring helper", () => {
  const { electron } = fakeShell();
  const mod = loadEmbeddedView(electron);
  // The WHOLE surface, not only the names main.ts imports: `embeddedWireNavEvents` has no importer
  // outside the module and is exported anyway, and a RENAMED export is a `TypeError` in a shell that no
  // suite here can start — the failure this package exists to catch before a release.
  assert.deepEqual(
    Object.keys(mod).sort(),
    [
      "embeddedGo", "embeddedNavigate", "embeddedRecover", "embeddedReload", "embeddedState",
      "embeddedViewBind", "embeddedViewEnsure", "embeddedViewPlace", "embeddedWireNavEvents",
      "embeddedZoom",
    ],
    "a new export is a new boundary — main.ts, preload.ts and the panel's fixture have to be told about it",
  );
});

test("ensure() builds the view hidden, with the security posture, and attaches it to the window", () => {
  const { electron, attached, permissions, mod } = boundModule();
  const view = mod.embeddedViewEnsure();
  assert.ok(view instanceof electron.WebContentsView, "ensure() must build electron's own WebContentsView");
  // The view loads arbitrary internet pages, so this is the posture and not a style: no `preload` key at
  // all (zero Node surface in the renderer), context isolation on, node integration off, sandbox on.
  assert.deepEqual(
    view.options.webPreferences,
    { contextIsolation: true, nodeIntegration: false, sandbox: true },
    "the view renders untrusted pages: contextIsolation on, nodeIntegration off, sandbox on, NO preload",
  );
  assert.equal(view.visible, false, "the view is added HIDDEN — the SPA reports bounds before it is shown");
  assert.deepEqual(attached, [view], "the view must be a child of the window's contentView");
  // The two hooks are installed inside a try/catch, so a rename would be silent: assert they were SET and
  // that they DENY by default rather than merely exist.
  assert.equal(typeof permissions.request, "function", "the permission REQUEST handler must be installed");
  assert.equal(typeof permissions.check, "function", "the permission CHECK handler must be installed");
  let granted = null;
  permissions.request(view.webContents, "media", (answer) => { granted = answer; });
  assert.equal(granted, false, "a permission request must be denied by default");
  assert.equal(permissions.check(view.webContents, "media"), false, "a permission check must be denied by default");
});

test("ensure() returns the view it already has while its webContents lives, and rebuilds a destroyed one", () => {
  const { mod } = boundModule();
  const first = mod.embeddedViewEnsure();
  assert.equal(mod.embeddedViewEnsure(), first, "a second ensure() must not stack a second view on the window");
  first.webContents.destroyed = true;
  const second = mod.embeddedViewEnsure();
  assert.notEqual(second, first, "a destroyed webContents cannot be loaded again — ensure() has to re-create it");
});

test("place() puts the view at the SPA's bounds and shows it", () => {
  const { mod } = boundModule();
  const bounds = { x: 1, y: 2, width: 800, height: 600 };
  assert.equal(policy.slotTooSmall(bounds), false, "guard: the crate calls this slot big enough to show a view in");
  mod.embeddedViewPlace(bounds);
  const view = mod.embeddedViewEnsure();
  assert.deepEqual(view.bounds, bounds, "the bounds are the SPA's CSS pixels, passed through unchanged");
  assert.equal(view.visible, true);
  assert.equal(mod.embeddedState().visible, true, "the state the SPA reads must agree with what was placed");
});

test("place() hides the view when the crate calls the slot too small — including an empty slot", () => {
  const { mod } = boundModule();
  const big = { x: 0, y: 0, width: 800, height: 600 };
  const small = { x: 0, y: 0, width: 10, height: 10 };
  assert.equal(policy.slotTooSmall(big), false, "guard: the big slot is a slot");
  assert.equal(policy.slotTooSmall(small), true, "guard: the small slot is under the crate's minimum");
  assert.equal(policy.slotTooSmall(null), true, "guard: an absent slot is too small");
  mod.embeddedViewPlace(big);
  const view = mod.embeddedViewEnsure();
  assert.equal(view.visible, true);
  mod.embeddedViewPlace(small);
  assert.equal(view.visible, false, "a too-small slot must HIDE the view, never leave it floating over the SPA");
  assert.equal(mod.embeddedState().visible, false, "and the state must say so");
  mod.embeddedViewPlace(big);
  mod.embeddedViewPlace(null);
  assert.equal(view.visible, false, "a null slot (the SPA clearing its placeholder) hides it the same way");
});

test("navigate() loads the ONE door's answer and records it — the raw string never reaches loadURL", () => {
  const { asked, mod } = boundModule();
  const raw = "https://example.com/x";
  const decided = urlPolicy.sanitizeBrowserUrl(raw);
  assert.deepEqual(mod.embeddedNavigate(raw), { ok: true, url: decided });
  assert.deepEqual(asked, [raw], "the door must be asked with the RAW string the SPA or the AI handed over");
  assert.deepEqual(mod.embeddedViewEnsure().webContents.loaded, [decided], "loadURL gets the door's answer");
  assert.equal(mod.embeddedState().url, decided, "the state record reports the URL that was loaded");
});

test("navigate() to a refused scheme loads the door's blank answer, never the raw javascript: target", () => {
  const raw = "javascript:alert(1)";
  const decided = urlPolicy.sanitizeBrowserUrl(raw);
  assert.notEqual(decided, raw, "guard: the crate REFUSES this scheme (otherwise this case pins nothing)");
  const { mod } = boundModule();
  assert.deepEqual(mod.embeddedNavigate(raw), { ok: true, url: decided });
  assert.deepEqual(mod.embeddedViewEnsure().webContents.loaded, [decided], "the refused target must not be loaded as written");
});

test("window.open() moves the SAME view for an allowed target and DROPS a refused one without blanking the page", () => {
  const { mod } = boundModule();
  const view = mod.embeddedViewEnsure();
  const reading = "https://example.com/";
  mod.embeddedNavigate(reading);
  // A `target=_blank` link is the norm on real sites and this is a SINGLE-TAB browser: the popup is
  // denied and the same view navigates instead.
  const allowed = "https://other.example/";
  assert.equal(policy.embeddedPopupTarget(urlPolicy.sanitizeBrowserUrl(allowed)), allowed, "guard: the crate passes an allowed target through");
  assert.deepEqual(view.webContents.openHandler({ url: allowed }), { action: "deny" }, "the popup itself is always denied");
  assert.deepEqual(view.webContents.loaded, [reading, allowed], "the SAME view navigates to the allowed target");
  // A refused scheme must NOT collapse the page being read to about:blank.
  const refused = "javascript:alert(1)";
  assert.equal(policy.embeddedPopupTarget(urlPolicy.sanitizeBrowserUrl(refused)), undefined, "guard: the crate DROPS a refused target");
  assert.deepEqual(view.webContents.openHandler({ url: refused }), { action: "deny" });
  assert.deepEqual(view.webContents.loaded, [reading, allowed], "a refused popup must not navigate the view at all");
  assert.equal(view.webContents.url, allowed, "and it must not blank what the operator is reading");
});

test("navigation events push the real state to the SPA as `embedded-browser:nav`", () => {
  const { mod, sent } = boundModule();
  const view = mod.embeddedViewEnsure();
  const url = mod.embeddedNavigate("https://example.com/x").url;
  view.webContents.title = "Example";
  view.webContents.history = { back: true, fwd: true };
  // All three events, because the address bar and the back/fwd buttons track the ACTUAL page: a
  // title-only update or an in-page (hash) navigation is a change the SPA has to see too.
  for (const event of ["did-navigate", "did-navigate-in-page", "page-title-updated"]) {
    view.webContents.emit(event);
    assert.deepEqual(
      sent.at(-1),
      ["embedded-browser:nav", { url, canBack: true, canFwd: true, title: "Example" }],
      `${event} must push url/canBack/canFwd/title to the SPA — exactly those four fields`,
    );
  }
});

test("a renderer crash hides the dead view and pushes `embedded-browser:gone` with the reason and code", () => {
  const { mod, sent } = boundModule();
  const view = mod.embeddedViewEnsure();
  mod.embeddedViewPlace({ x: 0, y: 0, width: 800, height: 600 });
  assert.equal(view.visible, true);
  view.webContents.emit("render-process-gone", null, { reason: "crashed", exitCode: 133 });
  assert.deepEqual(sent.at(-1), ["embedded-browser:gone", { reason: "crashed", exitCode: 133 }], "the SPA needs the reason to decide whether a reload can help");
  assert.equal(view.visible, false, "the dead view is HIDDEN so the SPA's recovery banner can paint in the slot");
  assert.equal(mod.embeddedState().visible, false, "and the state must stop claiming a live visible view");
});

test("a destroyed window is sent nothing — a nav push and a crash both check the window first", () => {
  const { mod, sent, win } = boundModule();
  const view = mod.embeddedViewEnsure();
  win.destroyed = true;
  view.webContents.emit("did-navigate");
  view.webContents.emit("render-process-gone", null, { reason: "crashed", exitCode: 1 });
  assert.deepEqual(sent, [], "send() on a destroyed window throws in Electron — the module must look before it pushes");
});

test("go/reload/zoom answer {ok:false} rather than throwing when there is no live view", () => {
  const { mod } = boundModule();
  // Every one of these is renderer-reachable, and the SPA may ask before its own Browser page has ever
  // been opened: a throw here becomes an IPC rejection the panel cannot tell apart from a dead shell.
  assert.deepEqual(mod.embeddedGo(-1), { ok: false });
  assert.deepEqual(mod.embeddedReload(), { ok: false });
  assert.deepEqual(mod.embeddedZoom(1), { ok: false });
  const view = mod.embeddedViewEnsure();
  view.webContents.destroyed = true;
  assert.deepEqual(mod.embeddedGo(1), { ok: false }, "a destroyed webContents is not a live view either");
  assert.deepEqual(mod.embeddedReload(), { ok: false });
  assert.deepEqual(mod.embeddedZoom(1), { ok: false });
});

test("go() drives the real history in the direction the crate's sign rule gives, and reload() the real load", () => {
  const { mod } = boundModule();
  const view = mod.embeddedViewEnsure();
  for (const delta of [-1, 1]) assert.deepEqual(mod.embeddedGo(delta), { ok: true });
  assert.deepEqual(
    view.webContents.went,
    [policy.goBackwards(-1) ? "back" : "fwd", policy.goBackwards(1) ? "back" : "fwd"],
    "the sign of the delta picks the direction — the channels are two, the rule is the crate's",
  );
  assert.deepEqual(mod.embeddedReload(), { ok: true });
  assert.equal(view.webContents.reloaded, 1, "reload() must reach the REAL webContents, not just answer ok");
});

test("zoom() applies the crate's clamp — a factor of 0 is 100%, not the 0.5 the floor would give", () => {
  const { mod } = boundModule();
  const view = mod.embeddedViewEnsure();
  // `Number(factor) || 1` BEFORE the clamp is the rule a hand-written Math.min/Math.max rewrite loses:
  // a 0 or a NaN factor is 100%, not the clamp's own floor of 50%. The crate's test is
  // `zoom_is_clamped_and_a_zero_means_one_hundred_percent`; the guard below is why this case is here.
  assert.equal(policy.zoomFactor(0), 1, "guard: the crate answers 100% for a zero factor");
  const cases = [0, 0.1, 1.5, 3, 99, Number.NaN];
  const factors = cases.map((factor) => mod.embeddedZoom(factor).factor);
  assert.deepEqual(factors, cases.map((factor) => policy.zoomFactor(factor)), "the module must apply the crate's answer, not its own arithmetic");
  assert.deepEqual(view.webContents.zooms, factors, "and the factor must reach the REAL webContents, in order");
  assert.deepEqual(mod.embeddedZoom(2), { ok: true, factor: policy.zoomFactor(2) }, "a zoom that applied answers its factor back");
});

test("recover() closes the dead view, removes it from the window, and reloads it in a fresh one", () => {
  const { attached, mod } = boundModule();
  const dead = mod.embeddedViewEnsure();
  const last = mod.embeddedNavigate("https://other.example/").url;
  assert.deepEqual(mod.embeddedRecover(), { ok: true });
  assert.equal(dead.webContents.closed, 1, "a crash-dead webContents is CLOSED — loadURL on it never recovers");
  assert.equal(dead.webContents.destroyed, true);
  const fresh = mod.embeddedViewEnsure();
  assert.notEqual(fresh, dead, "recovery builds a NEW view rather than reloading the dead one");
  assert.deepEqual(fresh.webContents.loaded, [policy.embeddedRecoverUrl(last)], "and loads the crate's recovery URL for the last decided one");
  assert.deepEqual(attached, [fresh], "the dead view is REMOVED from the contentView, not stacked under the fresh one");
});

test("recover() with nothing ever navigated loads the crate's blank fallback, never a third-party page", () => {
  const { mod } = boundModule();
  mod.embeddedViewEnsure();
  assert.deepEqual(mod.embeddedRecover(), { ok: true });
  // The regression this pins is the shell's own history: the view used to load a search engine eagerly,
  // which is a request to a third party on every launch. Recovery must not bring it back — and the
  // fallback's VALUE is the crate's (`embedded_recover_url`), asserted there, not restated here.
  assert.deepEqual(mod.embeddedViewEnsure().webContents.loaded, [policy.embeddedRecoverUrl(undefined)]);
});

test("recover() leaves the fresh view HIDDEN — the re-show block reads the flag ensure() has just cleared", () => {
  const { mod } = boundModule();
  mod.embeddedViewEnsure();
  mod.embeddedViewPlace({ x: 5, y: 6, width: 700, height: 500 });
  assert.deepEqual(mod.embeddedRecover(), { ok: true });
  const fresh = mod.embeddedViewEnsure();
  // PRE-EXISTING AND UNCHANGED, and measured before this file existed by the throwaway harness that
  // printed `recovered view re-shown = false | bounds = null`. `embeddedViewEnsure()` sets
  // `embeddedVisible = false` for the view it has just built, and `embeddedRecover`'s re-show block then
  // reads that flag — so its own `if (embeddedVisible && …)` cannot be true and the fresh view is left
  // for the SPA's next `place()` to show. This test PINS TODAY'S BEHAVIOUR, not an endorsement: the day
  // the re-show is fixed, these two assertions are the ones to flip, and the name above is the reason.
  assert.equal(fresh.visible, false, "the SPA's next place() is what shows a recovered view");
  assert.equal(fresh.bounds, null, "and the last placed bounds are not re-applied");
});

test("a window-less host is not a crash: the view is built, but nothing is attached and nothing is placed", () => {
  const { attached, mod } = boundModule({ withWindow: false });
  // `main.ts` re-creates the window and nulls it on close, so this is the state every operation can meet.
  const view = mod.embeddedViewEnsure();
  assert.deepEqual(attached, [], "there is no contentView yet, so there is nothing to attach to");
  mod.embeddedViewPlace({ x: 0, y: 0, width: 800, height: 600 });
  assert.equal(view.bounds, null, "place() without a window must not pretend the view is on screen");
  assert.equal(view.visible, false);
});

test("state() answers from the module's own record — it needs no host and no view to answer", () => {
  const { electron } = fakeShell();
  const mod = loadEmbeddedView(electron); // deliberately never bound
  assert.deepEqual(
    mod.embeddedState(),
    {
      ok: true,
      url: policy.shownUrl(undefined, "about:blank"),
      canBack: false,
      canFwd: false,
      title: "",
      visible: false,
    },
    "state() is the one entry point a renderer may call before the shell has bound anything — it must answer, not throw",
  );
});
