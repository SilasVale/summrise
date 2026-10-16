// THE BROWSER SLOT, EXECUTED — the committed module, a stubbed Electron, the real policy wasm.
//
// WHY THIS FILE EXISTS. `src/embedded-slot.ts` is the shell's own `WebContentsView` pattern, and this is
// its BROWSER instance: the view that loads arbitrary internet pages over the SPA's placeholder and is a
// first-class CDP target on :9333. It was verified exactly once by a throwaway smoke harness under `/tmp`
// — outside this repository, so the verification was not repeatable by CI and nothing went red the day
// one of these behaviours changed. The two tests that were here before it read SOURCE AS TEXT for a good
// reason (`test/ipc-door.test.mjs`: `main.ts` imports electron, and this package deliberately installs
// nothing — its `npm test` is `node --test` over node builtins), but the properties that were unpinned
// are RUNTIME ones: is the view attached hidden, does a crash hide the dead one and push `gone`, does a
// zoom of 0 end at 100%? A count over the text cannot answer any of them. So this file loads the module
// and runs it.
//
// THE MODULE WAS TWO MODULES, AND THIS FILE IS THE INSTANCE THAT SURVIVED THE MERGE. `embedded-view.ts`
// and `dsh-view.ts` were near-copies of one pattern; `embedded-slot.ts` is that pattern ONCE, as a
// factory, and `test/dsh-slot.test.mjs` is its other instance. The two suites were written BEFORE the
// unification (that was the point: the DSH cluster had no instrument at all), so they are what made the
// merge measurable rather than a rewrite — and every case in both of them is still here, re-pointed at
// the factory, with the ONE behaviour the merge had to decide (recover()'s re-show) flipped in place.
//
// WHAT IT LOADS, AND WHY THAT FILE. The committed `src/embedded-slot.js` — the CommonJS output that
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
// A FRESH MODULE AND A FRESH STUB PER TEST. Each test builds its OWN slot instance, and the slot keeps its
// view, url, bounds and visibility in that instance's closure, so a shared instance would make every test
// depend on the order the others ran in — and the state a test could not reach would be the one that hid a
// defect. Each test therefore gets a new stub AND a new evaluation of the source, the move
// `gateway/wasm/run-cases.mjs` already makes and names at line 173: "a fresh instance is also what a new
// isolate is."
//
// WHAT IT DECIDES: NOTHING, AND THAT IS THE POINT. This file pins the module's WIRING — which platform
// call, with which argument, at which moment — and every expected VALUE is read back out of the policy
// crates the shell ships (`summrise_shell_policy.js`, `summrise_url_policy.js`): the zoom clamp, the
// minimum slot, the popup drop rule, the shown-url fallback and the recovery URL are asked of the crates
// rather than restated here, so a policy change cannot leave this file asserting yesterday's answer, and a
// value disagreement is the crates' business. Each case opens with a `guard:` assertion that the crate
// really answers what makes the case interesting, so a case cannot pass by measuring nothing.
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
const SLOT_JS = path.join(SRC, "embedded-slot.js");

const require = createRequire(import.meta.url);
// THE TWO POLICY CRATES, as the shell ships them — the source of every expected value below.
const policy = require(path.join(SRC, "summrise_shell_policy.js"));
const urlPolicy = require(path.join(SRC, "summrise_url_policy.js"));

/** The Electron surface `embedded-slot.js` touches, and NOTHING besides it: every method below exists
 *  because the module calls it, so this object is that module's platform boundary read off the module
 *  rather than invented beside it. `attached` models a real `contentView`: re-adding a view MOVES it
 *  (it does not stack a copy), which is how the module keeps the view above the SPA's placeholder. */
function fakeShell({ withWindow = true } = {}) {
  const attached = [];
  const sent = [];
  const views = [];
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

  return { electron, win: withWindow ? win : null, attached, sent, views, permissions };
}

/** The committed module, evaluated against `electron` — a CommonJS module with its one bare specifier
 *  answered by the stub and every other specifier resolved by the real loader, from `src/`. */
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

/** The BROWSER's config, bound the way `main.ts` binds it: the ONE load door IS the url policy's
 *  `sanitizeBrowserUrl`, wrapped so the test can see the RAW string the slot handed it; the popup target
 *  goes through that same door; the state's url falls back to the last decided one; and the slot has the
 *  nav channel that brings the browser's history surface. */
function browserConfig(shell, asked) {
  const door = (raw) => { asked.push(raw); return urlPolicy.sanitizeBrowserUrl(raw); };
  return {
    config: {
      win: () => shell.win,
      door,
      popupTarget: (raw) => policy.embeddedPopupTarget(door(raw)),
      recoverUrl: (lastDecided) => policy.embeddedRecoverUrl(lastDecided),
      stateUrl: (live, lastDecided) => policy.shownUrl(live, lastDecided),
      goneChannel: "embedded-browser:gone",
      navChannel: "embedded-browser:nav",
    },
    door,
  };
}

/** A bound browser slot over a fresh stub. */
function boundModule(options) {
  const shell = fakeShell(options);
  const mod = loadSlotModule(shell.electron);
  const asked = [];
  const { config } = browserConfig(shell, asked);
  return { ...shell, mod, asked, slot: mod.embeddedSlot(config) };
}

test("a slot cannot be built without its host — the factory REFUSES, and it names the missing piece", () => {
  const shell = fakeShell();
  const mod = loadSlotModule(shell.electron);
  const asked = [];
  const { config } = browserConfig(shell, asked);
  // The two modules this replaces bound their host once and threw `…Bind() was never called` from every
  // operation until they did. The rule is kept and the moment moved EARLIER: a config missing a piece is
  // refused at construction, so no half-built slot can exist and no operation has to guard.
  assert.throws(() => mod.embeddedSlot(), /embedded-slot: a slot is built from its host config/, "no config at all is refused");
  for (const missing of ["win", "door", "popupTarget", "recoverUrl", "stateUrl"]) {
    const broken = { ...config };
    delete broken[missing];
    assert.throws(
      () => mod.embeddedSlot(broken),
      new RegExp(`embedded-slot: ${missing} is not a function`),
      `${missing} must be NAMED in the refusal — a slot that cannot attach, load or recover must not be built quietly`,
    );
  }
  assert.throws(() => mod.embeddedSlot({ ...config, goneChannel: "" }), /embedded-slot: goneChannel is missing/);
  assert.throws(() => mod.embeddedSlot({ ...config, navChannel: "" }), /embedded-slot: navChannel is present but is not a channel name/);
  assert.deepEqual(shell.views, [], "a refused construction builds NOTHING — there is no view left to leak");
});

test("the factory exports ONE function, and the browser instance's surface is these eight", () => {
  const { electron, mod, slot } = boundModule();
  // The WHOLE surface, not only the names main.ts imports: a RENAMED export is a `TypeError` in a shell
  // that no suite here can start — the failure this package exists to catch before a release. The factory
  // is now the module's only export, and the INSTANCE is the boundary `main.ts` destructures: the bind
  // helper and the nav-wiring helper the two old modules exported are configuration now.
  assert.deepEqual(Object.keys(mod).sort(), ["embeddedSlot"], "a new export is a new boundary — main.ts, preload.ts and the panel's fixture have to be told about it");
  assert.deepEqual(
    Object.keys(slot).sort(),
    ["ensure", "go", "navigate", "place", "recover", "reload", "state", "zoom"],
    "the browser's slot is the one WITH the history surface: go/zoom exist only where a navChannel was given",
  );
  assert.equal(typeof mod.embeddedSlot, "function");
  for (const [name, fn] of Object.entries(slot)) assert.equal(typeof fn, "function", `${name} must be a function`);
});

test("ensure() builds the view hidden, with the security posture, and attaches it to the window", () => {
  const { electron, attached, permissions, slot } = boundModule();
  const view = slot.ensure();
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
  const { slot } = boundModule();
  const first = slot.ensure();
  assert.equal(slot.ensure(), first, "a second ensure() must not stack a second view on the window");
  first.webContents.destroyed = true;
  const second = slot.ensure();
  assert.notEqual(second, first, "a destroyed webContents cannot be loaded again — ensure() has to re-create it");
});

test("place() puts the view at the SPA's bounds and shows it", () => {
  const { slot } = boundModule();
  const bounds = { x: 1, y: 2, width: 800, height: 600 };
  assert.equal(policy.slotTooSmall(bounds), false, "guard: the crate calls this slot big enough to show a view in");
  slot.place(bounds);
  const view = slot.ensure();
  assert.deepEqual(view.bounds, bounds, "the bounds are the SPA's CSS pixels, passed through unchanged");
  assert.equal(view.visible, true);
  assert.equal(slot.state().visible, true, "the state the SPA reads must agree with what was placed");
});

test("place() hides the view when the crate calls the slot too small — including an empty slot", () => {
  const { slot } = boundModule();
  const big = { x: 0, y: 0, width: 800, height: 600 };
  const small = { x: 0, y: 0, width: 10, height: 10 };
  assert.equal(policy.slotTooSmall(big), false, "guard: the big slot is a slot");
  assert.equal(policy.slotTooSmall(small), true, "guard: the small slot is under the crate's minimum");
  assert.equal(policy.slotTooSmall(null), true, "guard: an absent slot is too small");
  slot.place(big);
  const view = slot.ensure();
  assert.equal(view.visible, true);
  slot.place(small);
  assert.equal(view.visible, false, "a too-small slot must HIDE the view, never leave it floating over the SPA");
  assert.equal(slot.state().visible, false, "and the state must say so");
  slot.place(big);
  slot.place(null);
  assert.equal(view.visible, false, "a null slot (the SPA clearing its placeholder) hides it the same way");
});

test("navigate() loads the ONE door's answer and records it — the raw string never reaches loadURL", () => {
  const { asked, slot } = boundModule();
  const raw = "https://example.com/x";
  const decided = urlPolicy.sanitizeBrowserUrl(raw);
  assert.deepEqual(slot.navigate(raw), { ok: true, url: decided });
  assert.deepEqual(asked, [raw], "the door must be asked with the RAW string the SPA or the AI handed over");
  assert.deepEqual(slot.ensure().webContents.loaded, [decided], "loadURL gets the door's answer");
  assert.equal(slot.state().url, decided, "the state record reports the URL that was loaded");
});

test("navigate() to a refused scheme loads the door's blank answer, never the raw javascript: target", () => {
  const raw = "javascript:alert(1)";
  const decided = urlPolicy.sanitizeBrowserUrl(raw);
  assert.notEqual(decided, raw, "guard: the crate REFUSES this scheme (otherwise this case pins nothing)");
  const { slot } = boundModule();
  assert.deepEqual(slot.navigate(raw), { ok: true, url: decided });
  assert.deepEqual(slot.ensure().webContents.loaded, [decided], "the refused target must not be loaded as written");
});

test("window.open() moves the SAME view for an allowed target and DROPS a refused one without blanking the page", () => {
  const { slot } = boundModule();
  const view = slot.ensure();
  const reading = "https://example.com/";
  slot.navigate(reading);
  // A `target=_blank` link is the norm on real sites and this is a SINGLE-TAB browser: the popup is
  // denied and the same view navigates instead.
  const allowed = "https://other.example/";
  const decidedAllowed = urlPolicy.sanitizeBrowserUrl(allowed);
  assert.equal(policy.embeddedPopupTarget(decidedAllowed), allowed, "guard: the crate passes an allowed target through");
  // THE DOOR IS IDEMPOTENT, WHICH IS WHY DECIDING ONCE IS DECIDING TWICE: the popup decision hands the
  // DECIDED target to the view, so this case would pin a different answer if re-deciding changed it.
  assert.equal(urlPolicy.sanitizeBrowserUrl(decidedAllowed), decidedAllowed, "guard: the door's answer is a fixed point");
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
  const { slot, sent } = boundModule();
  const view = slot.ensure();
  const url = slot.navigate("https://example.com/x").url;
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
  const { slot, sent } = boundModule();
  const view = slot.ensure();
  slot.place({ x: 0, y: 0, width: 800, height: 600 });
  assert.equal(view.visible, true);
  view.webContents.emit("render-process-gone", null, { reason: "crashed", exitCode: 133 });
  assert.deepEqual(sent.at(-1), ["embedded-browser:gone", { reason: "crashed", exitCode: 133 }], "the SPA needs the reason to decide whether a reload can help");
  assert.equal(view.visible, false, "the dead view is HIDDEN so the SPA's recovery banner can paint in the slot");
  assert.equal(slot.state().visible, false, "and the state must stop claiming a live visible view");
});

test("a destroyed window is sent nothing — and the dead view is hidden anyway (the hide does not wait on the window)", () => {
  const { slot, sent, win } = boundModule();
  const view = slot.ensure();
  slot.place({ x: 0, y: 0, width: 800, height: 600 });
  win.destroyed = true;
  view.webContents.emit("did-navigate");
  view.webContents.emit("render-process-gone", null, { reason: "crashed", exitCode: 1 });
  assert.deepEqual(sent, [], "send() on a destroyed window throws in Electron — the module must look before it pushes");
  // THE ORDER IS THE SLOT'S, AND IT IS THE SAME FOR BOTH INSTANCES (this was the one behaviour the two
  // modules disagreed about without saying so): the hide needs nothing from the host, so a renderer that
  // died is taken off the screen even when its window is already gone. A window-less pane showing a dead
  // view's last frame is worse than a pane that admits it has nothing.
  assert.equal(view.visible, false, "the hide is not conditional on the window surviving");
  assert.equal(slot.state().visible, false, "and the record agrees");
});

test("go/reload/zoom answer {ok:false} rather than throwing when there is no live view", () => {
  const { slot } = boundModule();
  // Every one of these is renderer-reachable, and the SPA may ask before its own Browser page has ever
  // been opened: a throw here becomes an IPC rejection the panel cannot tell apart from a dead shell.
  assert.deepEqual(slot.go(-1), { ok: false });
  assert.deepEqual(slot.reload(), { ok: false });
  assert.deepEqual(slot.zoom(1), { ok: false });
  const view = slot.ensure();
  view.webContents.destroyed = true;
  assert.deepEqual(slot.go(1), { ok: false }, "a destroyed webContents is not a live view either");
  assert.deepEqual(slot.reload(), { ok: false });
  assert.deepEqual(slot.zoom(1), { ok: false });
});

test("go() drives the real history in the direction the crate's sign rule gives, and reload() the real load", () => {
  const { slot } = boundModule();
  const view = slot.ensure();
  for (const delta of [-1, 1]) assert.deepEqual(slot.go(delta), { ok: true });
  assert.deepEqual(
    view.webContents.went,
    [policy.goBackwards(-1) ? "back" : "fwd", policy.goBackwards(1) ? "back" : "fwd"],
    "the sign of the delta picks the direction — the channels are two, the rule is the crate's",
  );
  assert.deepEqual(slot.reload(), { ok: true });
  assert.equal(view.webContents.reloaded, 1, "reload() must reach the REAL webContents, not just answer ok");
});

test("zoom() applies the crate's clamp — a factor of 0 is 100%, not the 0.5 the floor would give", () => {
  const { slot } = boundModule();
  const view = slot.ensure();
  // `Number(factor) || 1` BEFORE the clamp is the rule a hand-written Math.min/Math.max rewrite loses:
  // a 0 or a NaN factor is 100%, not the clamp's own floor of 50%. The crate's test is
  // `zoom_is_clamped_and_a_zero_means_one_hundred_percent`; the guard below is why this case is here.
  assert.equal(policy.zoomFactor(0), 1, "guard: the crate answers 100% for a zero factor");
  const cases = [0, 0.1, 1.5, 3, 99, Number.NaN];
  const factors = cases.map((factor) => slot.zoom(factor).factor);
  assert.deepEqual(factors, cases.map((factor) => policy.zoomFactor(factor)), "the module must apply the crate's answer, not its own arithmetic");
  assert.deepEqual(view.webContents.zooms, factors, "and the factor must reach the REAL webContents, in order");
  assert.deepEqual(slot.zoom(2), { ok: true, factor: policy.zoomFactor(2) }, "a zoom that applied answers its factor back");
});

test("recover() closes the dead view, removes it from the window, and reloads it in a fresh one", () => {
  const { attached, slot } = boundModule();
  const dead = slot.ensure();
  const last = slot.navigate("https://other.example/").url;
  assert.deepEqual(slot.recover(), { ok: true });
  assert.equal(dead.webContents.closed, 1, "a crash-dead webContents is CLOSED — loadURL on it never recovers");
  assert.equal(dead.webContents.destroyed, true);
  const fresh = slot.ensure();
  assert.notEqual(fresh, dead, "recovery builds a NEW view rather than reloading the dead one");
  assert.deepEqual(fresh.webContents.loaded, [policy.embeddedRecoverUrl(last)], "and loads the crate's recovery URL for the last decided one");
  assert.deepEqual(attached, [fresh], "the dead view is REMOVED from the contentView, not stacked under the fresh one");
});

test("recover() with nothing ever navigated loads the crate's blank fallback, never a third-party page", () => {
  const { slot } = boundModule();
  slot.ensure();
  assert.deepEqual(slot.recover(), { ok: true });
  // The regression this pins is the shell's own history: the view used to load a search engine eagerly,
  // which is a request to a third party on every launch. Recovery must not bring it back — and the
  // fallback's VALUE is the crate's (`embedded_recover_url`), asserted there, not restated here.
  assert.deepEqual(slot.ensure().webContents.loaded, [policy.embeddedRecoverUrl(undefined)]);
});

test("recover() RE-SHOWS a fresh view at the slot's last placement, and leaves a hidden one hidden", () => {
  const { slot } = boundModule();
  const bounds = { x: 5, y: 6, width: 700, height: 500 };
  slot.ensure();
  slot.place(bounds);
  assert.equal(slot.state().visible, true, "guard: the slot was LIVE when the renderer died");
  assert.deepEqual(slot.recover(), { ok: true });
  const fresh = slot.ensure();
  // THE ONE BEHAVIOUR THE UNIFICATION HAD TO DECIDE, and it is decided toward the block's own stated
  // purpose. Both old modules kept their last bounds "so a renderer-crash recovery can re-show the fresh
  // view at the same spot without waiting for the SPA", and both then read the visible flag that
  // `ensure()` had just cleared — so the block could not run. The placement is read BEFORE the re-create
  // here, which is what makes it reachable, and the flag is restored so the record agrees with the screen.
  // This test asserted the OLD (unreachable) behaviour until the unification; it was flipped in the same
  // change rather than left describing a branch that no longer exists.
  assert.equal(fresh.visible, true, "a live slot's recovery re-shows the fresh view at the same spot");
  assert.deepEqual(fresh.bounds, bounds, "and re-applies the placement the SPA last reported");
  assert.equal(slot.state().visible, true, "the state the SPA reads agrees with the screen");
  // ...AND A SLOT THAT WAS NOT LIVE STAYS HIDDEN: the SPA's own place() is what shows a view it had
  // already hidden, so recovery must not resurrect one behind its back.
  slot.place(null);
  fresh.webContents.destroyed = true;
  assert.deepEqual(slot.recover(), { ok: true });
  const hidden = slot.ensure();
  assert.equal(hidden.visible, false, "a slot the SPA had hidden is left hidden by recovery");
  assert.equal(hidden.bounds, null, "and no bounds are re-applied to it");
});

test("a window-less host is not a crash: the view is built, but nothing is attached and nothing is placed", () => {
  const { attached, slot } = boundModule({ withWindow: false });
  // `main.ts` re-creates the window and nulls it on close, so this is the state every operation can meet.
  const view = slot.ensure();
  assert.deepEqual(attached, [], "there is no contentView yet, so there is nothing to attach to");
  slot.place({ x: 0, y: 0, width: 800, height: 600 });
  assert.equal(view.bounds, null, "place() without a window must not pretend the view is on screen");
  assert.equal(view.visible, false);
});

test("state() answers from the slot's own record — it never reads the window", () => {
  const shell = fakeShell();
  const mod = loadSlotModule(shell.electron);
  const asked = [];
  const { config } = browserConfig(shell, asked);
  // A window getter that REFUSES: `state()` is the one entry point a renderer may call before the shell
  // has a window at all, so reading it here would be the whole failure (an IPC rejection the panel cannot
  // tell apart from a dead shell) rather than a detail.
  const slot = mod.embeddedSlot({ ...config, win: () => { throw new Error("state() must not read the window"); } });
  assert.deepEqual(
    slot.state(),
    {
      ok: true,
      url: policy.shownUrl(undefined, "about:blank"),
      canBack: false,
      canFwd: false,
      title: "",
      visible: false,
    },
    "state() must answer, not throw, for a slot that has never been placed or navigated",
  );
  assert.deepEqual(shell.views, [], "and answering must not build a view either");
});
