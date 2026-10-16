// THE DSH SLOT, EXECUTED — the committed module, a stubbed Electron, the real policy wasm.
//
// WHY THIS FILE EXISTS, AND WHY IT IS THE REASON THE UNIFICATION COULD HAPPEN AT ALL. The DSH view is the
// shell's SECOND `WebContentsView` (the embedded real-render browser is the first) and it was verified by
// nothing: it lived inside `main.ts`, which no suite in this package can import because importing it
// starts the whole application, so the properties below had no instrument at all. The recorded next move
// was to UNIFY it with the browser view — a function-by-function comparison found the two to be
// near-copies, and the shared core became ONE parametrised factory (`src/embedded-slot.ts`) — and a
// unification performed before these behaviours were pinned would have been a rewrite of two views
// measured against nothing. So this file came first, and it is what made the merge measurable: every case
// below is the one it has always been, re-pointed at the factory, plus the ONE behaviour the merge had to
// decide (recover()'s re-show), flipped in the same change as the module.
//
// WHAT IT LOADS, AND WHY THAT FILE. The committed `src/embedded-slot.js` — the CommonJS output that
// `src/main.js` requires and the release ships — not the `.ts`. CI already holds the pair together (the
// `electron src freshness` step re-runs `tsc` and `cmp`s the result against the committed `.js`), and the
// artifact worth executing is the one that reaches the operator's device.
//
// HOW IT LOADS IT WITHOUT INSTALLING ELECTRON. A CommonJS module needs four things — `exports`,
// `require`, `module`, `__filename`/`__dirname` — so this file supplies them and evaluates the source
// with its own `require`: the ONE specifier that has to be intercepted is the bare `electron`, and
// everything else (`./summrise_shell_policy`) goes through the real loader rooted at `src/`, so the
// module and this test share ONE instance of the real policy wasm. The module's text is never edited,
// regexed or shimmed.
//
// A FRESH MODULE AND A FRESH STUB PER TEST. Each test builds its OWN slot instance, and the slot keeps its
// view, its visibility, its bounds and its last decided url in that instance's closure, so a shared
// instance would make every test depend on the order the others ran in — and the state a test could not
// reach would be the one that hid a defect. Each test therefore gets a new stub AND a new evaluation of
// the source, the move `gateway/wasm/run-cases.mjs` already makes and names at line 173: "a fresh
// instance is also what a new isolate is."
//
// THE CONFIG IS THE WAY `main.ts` CONFIGURES IT, and its entries are the shell's own: the window is a
// getter (the shell re-creates it and nulls it on close), `door` is `main.ts`'s `dshTargetFor` (the url
// policy's admission composed with the shell policy's serialization), `popupTarget` is the same door's
// answer through `dshPopupTarget`, and `recoverUrl`/`stateUrl` are the DSH's own answers (`dshHome`, and a
// record that describes the live view only). What this file CANNOT prove is that `main.ts` builds it this
// way: that is `test/ipc-door.test.mjs`'s source scan, and the two files exist together because neither
// can see the other's half.
//
// WHAT IT DECIDES: NOTHING, AND THAT IS THE POINT. Every expected VALUE below is read back out of the
// policy crates the shell ships (`summrise_shell_policy.js`, `summrise_url_policy.js`): the harness
// home, the minimum slot, the popup drop rule and the door's refusal are asked of the crates rather
// than restated here, so a policy change cannot leave this file asserting yesterday's answer. Each case
// opens with a `guard:` assertion that the crate really answers what makes the case interesting, so a
// case cannot pass by measuring nothing.
//
// WHAT IT CANNOT SEE, and no test here can: real Chromium compositing, the loopback HTTP the harness
// actually serves, and whether the committed `.wasm` LOADS — that last one is
// `test/shell-policy-wasm.test.mjs` and `test/url-policy-wasm.test.mjs`, which execute the artifacts
// directly. This file's stub is the platform, and the only claim it makes about Electron is the shape
// of the calls the module makes on it.
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

/** A port that is NOT the crate's default, so a module that hard-coded the harness's address instead of
 *  asking `dshBase()` would fail here rather than pass in the one deployment this box happens to run. */
const PORT = 18999;
const EXTRA_PORT = 18998;

/** A window's two surfaces, and NOTHING besides them: `contentView` is where a view is attached (and
 *  re-adding MOVES rather than stacks, which is how the module keeps the view above the SPA's slot),
 *  and `webContents.send` is the one channel out to the renderer. */
function fakeWindow() {
  const attached = [];
  const sent = [];
  return {
    attached,
    sent,
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
}

/** The Electron surface `embedded-slot.js` touches, and NOTHING besides it: every method below exists
 *  because the module calls it, so this object is that module's platform boundary read off the module
 *  rather than invented beside it. `views` is every view the module built, in creation order — the DSH
 *  view is re-created on recovery, so the stub records instances instead of a last one. */
function fakeShell({ withWindow = true } = {}) {
  const views = [];
  const win = withWindow ? fakeWindow() : null;
  const permissions = { request: null, check: null };

  class WebContents {
    constructor() {
      this.destroyed = false;
      this.url = "about:blank";
      this.title = "";
      this.loaded = [];
      this.reloaded = 0;
      this.closed = 0;
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
    reload() { this.reloaded += 1; }
    close() { this.closed += 1; this.destroyed = true; }
  }

  class WebContentsView {
    constructor(options) { this.options = options; this.webContents = new WebContents(); this.visible = null; this.bounds = null; views.push(this); }
    setVisible(visible) { this.visible = visible; }
    setBounds(bounds) { this.bounds = bounds; }
  }

  const electron = {
    WebContentsView,
    BrowserWindow: class {},
    // The DSH's view installs the deny-by-default permission pair too — the security asymmetry the
    // unification resolved. The stub records the handlers so the suite can assert they were SET and that
    // they DENY, rather than merely exist behind a try/catch.
    session: {
      defaultSession: {
        setPermissionRequestHandler(handler) { permissions.request = handler; },
        setPermissionCheckHandler(handler) { permissions.check = handler; },
      },
    },
  };

  return { electron, win, views, permissions, newWindow: () => fakeWindow() };
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

/** The DSH's config, bound the way `main.ts` binds it: the ONE door IS the shell's `dshTarget`
 *  over the url policy's `isDshUrl`, wrapped so the test can see the RAW string the slot handed it — the
 *  same way `embedded-view.test.mjs` wraps `sanitizeBrowserUrl`. NO `navChannel`: the pane has no address
 *  bar and no back/fwd buttons. */
function dshConfig(shell, asked, overrides = {}) {
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

/** A bound DSH slot over a fresh stub. */
function boundModule(options) {
  const shell = fakeShell(options);
  const mod = loadSlotModule(shell.electron);
  const asked = [];
  urlPolicy.setDshPort(PORT);
  return Object.assign(shell, { mod, asked, slot: mod.embeddedSlot(dshConfig(shell, asked)) });
}

/** The address `open()` and `recover()` must load, ASKED OF THE CRATE — and a guard that the port this
 *  file set is the one the crate answers with, so neither case can pass against a hard-coded address. */
function home() {
  urlPolicy.setDshPort(PORT);
  const base = urlPolicy.dshBase();
  assert.equal(base, `http://127.0.0.1:${PORT}`, "guard: setDshPort really moved the crate's DSH base off its default");
  return policy.dshHome(base);
}

/** `main.ts`'s `dshOpen`: the OPEN OPERATION IS THE CALLER'S (the lifecycle — create lazily, then navigate
 *  home — is what makes the two instances differ), so this file composes it the way that file does. */
function open(slot) {
  return slot.navigate(home());
}

test("a slot cannot be built without its host — the factory REFUSES, and it names the missing piece", () => {
  const shell = fakeShell();
  const mod = loadSlotModule(shell.electron);
  // The old seam bound its host once and threw `dshViewBind() was never called` from every operation until
  // it did. The rule is kept and the moment moved EARLIER: a config missing a piece is refused at
  // construction, so no operation has to guard and no half-built slot can exist.
  assert.throws(() => mod.embeddedSlot(), /embedded-slot: a slot is built from its host config/, "no config at all is refused");
  for (const missing of ["win", "door", "popupTarget", "recoverUrl", "stateUrl"]) {
    const broken = dshConfig(shell, []);
    delete broken[missing];
    assert.throws(
      () => mod.embeddedSlot(broken),
      new RegExp(`embedded-slot: ${missing} is not a function`),
      `${missing} must be NAMED in the refusal`,
    );
  }
  assert.throws(() => mod.embeddedSlot({ ...dshConfig(shell, []), goneChannel: "" }), /embedded-slot: goneChannel is missing/);
});

test("an unbound slot creates NOTHING and leaves NO state — the refusal precedes every construction", () => {
  // WHAT THIS CASE USED TO PIN, AND WHAT WAS WRONG WITH IT: it proved a refused CONSTRUCTION builds
  // nothing, and its own comment claimed the leaked-view shape the old seam had was "unreachable — the
  // config is checked before anything is constructed". THAT CLAIM WAS FALSE. The window getter is a host
  // dependency read at USE time, and `ensure()` read it AFTER `view = built`, so a getter that REFUSED —
  // an unbound host, `deps().win()` on a slot whose bind never ran — still left a fully built view in the
  // slot's state: no window ever showed it, its crash handler was live, a later `state()`/`reload()`
  // answered about it, and a SECOND `ensure()` returned it without reading the window at all. The host is
  // now read BEFORE the build, which is a DELIBERATE BEHAVIOUR CHANGE ON THE UNBOUND PATH ONLY (a bound
  // host's getter is called once and returns exactly what it returned before), and this case is
  // re-expressed to require the property it used to claim. NOTHING IS DELETED: the construction refusal
  // below stays — it is still true — and the use-time refusal is what makes the claim true at last.
  const shell = fakeShell();
  const mod = loadSlotModule(shell.electron);
  // ── A REFUSED CONSTRUCTION BUILDS NOTHING ─────────────────────────────────────────────────────────
  for (const missing of ["win", "door", "popupTarget", "recoverUrl", "stateUrl"]) {
    const broken = dshConfig(shell, []);
    delete broken[missing];
    assert.throws(() => mod.embeddedSlot(broken), new RegExp(`embedded-slot: ${missing}`));
    assert.deepEqual(shell.views, [], `a slot refused for a missing ${missing} must build no view at all`);
  }
  assert.throws(() => mod.embeddedSlot(), /embedded-slot:/);
  assert.deepEqual(shell.views, [], "and the config-less refusal builds nothing either");
  // ── ...AND NEITHER DOES A REFUSED `ensure()` — THE PATH THAT COULD LEAK ────────────────────────────
  const asked = [];
  let refuse = false;
  const slot = mod.embeddedSlot(dshConfig(shell, asked, {
    win: () => { if (refuse) throw new Error("dshViewBind() was never called"); return shell.win; },
  }));
  refuse = true;
  const LOUD = /dshViewBind\(\) was never called/;
  assert.throws(() => slot.ensure(), LOUD, "the refusal is still LOUD: a slot with no host does not fall back");
  assert.deepEqual(shell.views, [], "and it built NOTHING — no view is constructed before the host is read");
  assert.throws(() => slot.ensure(), LOUD, "a second call refuses AGAIN rather than returning the view the first one built");
  assert.deepEqual(shell.views, [], "so there is still no view to return");
  // NO STATE BEHIND IT: the reads answer from an empty slot, exactly as they do before a view exists, and
  // the record describes no view rather than an orphan nobody ever showed.
  assert.deepEqual(slot.reload(), { ok: false }, "reload has no view to reach");
  assert.deepEqual(
    slot.state(),
    { ok: true, url: policy.shownUrl(undefined, ""), title: "", visible: false },
    "and the record is the empty one — not a description of a view that was never attached",
  );
  assert.equal(shell.win.attached.length, 0, "nor did any view reach the window's contentView");
  // EVERY ENTRY POINT THAT BUILDS REFUSES THE SAME WAY, and not one of them leaves a view behind.
  assert.throws(() => slot.place({ x: 0, y: 0, width: 800, height: 600 }), LOUD, "place() dies on the same refusal");
  assert.throws(() => open(slot), LOUD, "navigate() too: the door runs, then ensure() refuses");
  assert.throws(() => slot.recover(), LOUD, "and so does the operation whose whole job is to BUILD a view");
  assert.deepEqual(shell.views, [], "place(), navigate() and recover() each built nothing");
  assert.equal(shell.win.attached.length, 0, "and attached nothing");
  assert.deepEqual(shell.win.sent, [], "and pushed nothing");
});

test("the hide does not wait on the window: a getter that refuses still leaves the dead view hidden", () => {
  // The ORDER inside the crash handler, from the side a reader would not guess: the window is read only
  // for the PUSH, and the hide happens before it — so a host that has stopped answering (this getter) still
  // gets the dead view off the screen, and the throw is on the push. Unifying decided this order FOR BOTH
  // instances (the browser's handler used to return before hiding), which is why it is pinned here.
  const shell = fakeShell();
  const mod = loadSlotModule(shell.electron);
  let refuse = false;
  const slot = mod.embeddedSlot(dshConfig(shell, [], {
    win: () => { if (refuse) throw new Error("this host has stopped answering"); return shell.win; },
  }));
  open(slot);
  slot.place({ x: 0, y: 0, width: 800, height: 600 });
  const view = shell.views[0];
  assert.equal(view.visible, true, "guard: the slot was showing before the crash");
  refuse = true;
  assert.throws(
    () => view.webContents.emit("render-process-gone", {}, { reason: "crashed", exitCode: 1 }),
    /this host has stopped answering/,
    "the push needs the window, so a refusing getter throws from inside the handler",
  );
  assert.equal(view.visible, false, "the hide needs nothing from the host, so it ran before the throw");
  assert.deepEqual(shell.win.sent, [], "and nothing was pushed");
});

test("the factory exports ONE function, and the DSH instance's surface is these six — no history, so no go/zoom", () => {
  const { electron, mod, slot } = boundModule();
  // The WHOLE surface, not only the names main.ts imports: a RENAMED export is a `TypeError` in a shell
  // that no suite here can start — the failure this package exists to catch before a release. `ensure` IS
  // on the instance (the browser's caller calls it eagerly at startup; this pane's never does — the
  // lifecycle is the CALLER's), and `go`/`zoom` are NOT: they come with a navChannel, and this slot has
  // none, so there is no channel the preload exposes for them to be uncalled surface beside.
  assert.deepEqual(Object.keys(mod).sort(), ["embeddedSlot"], "the factory is the module's only export");
  assert.deepEqual(
    Object.keys(slot).sort(),
    ["ensure", "navigate", "place", "recover", "reload", "state"],
    "the DSH's slot has no go/zoom: a pane with no back/fwd buttons must not carry the channels",
  );
  assert.equal("go" in slot, false);
  assert.equal("zoom" in slot, false);
  assert.equal(typeof electron, "object", "guard: the stub is the platform this ran against");
});

test("open() builds the view LAZILY, hidden, with the browser view's security posture, and loads the harness home", () => {
  const { slot, views, win, asked, permissions } = boundModule();
  // LAZY: reading the state must not build a view — the SPA asks for state on a page the operator may
  // never open, and a view per page would be a Chromium process per page.
  assert.deepEqual(slot.state(), { ok: true, url: "", title: "", visible: false }, "state() before any open");
  assert.equal(views.length, 0, "state() must not create the view");
  const expected = home();
  assert.deepEqual(open(slot), { ok: true, url: expected }, "open() answers with the address it loaded");
  assert.equal(views.length, 1, "open() creates the view — the slot builds it inside navigate()");
  const view = views[0];
  // The DSH is the operator's own harness served over loopback — still a whole web application, so the
  // posture is the embedded view's: no `preload` key at all (zero Node surface), isolation on, node
  // integration off, sandbox on.
  assert.deepEqual(
    view.options.webPreferences,
    { contextIsolation: true, nodeIntegration: false, sandbox: true },
    "the DSH SPA needs no Node surface: contextIsolation on, nodeIntegration off, sandbox on, NO preload",
  );
  assert.equal(view.visible, false, "the view is added HIDDEN — the SPA reports bounds before it is shown");
  assert.deepEqual(win.attached, [view], "the view must be a child of the window's contentView");
  assert.deepEqual(asked, [expected], "the home address goes through the ONE door, like every other address");
  assert.deepEqual(view.webContents.loaded, [expected], "and the door's answer is what is loaded");
  // THE PERMISSION PAIR IS INSTALLED FOR THIS SLOT TOO — the one behaviour this suite gained from the
  // unification, and a SECURITY change rather than a tidy-up: the browser's view denied by default from the
  // start, and the DSH's inherited whatever the default session allows for a view that renders the harness
  // UI. Deny-by-default is the safer side, so it is the shared one.
  assert.equal(typeof permissions.request, "function", "the permission REQUEST handler must be installed here too");
  assert.equal(typeof permissions.check, "function", "the permission CHECK handler must be installed here too");
  let granted = null;
  permissions.request(view.webContents, "media", (answer) => { granted = answer; });
  assert.equal(granted, false, "a permission request on the DSH's view is denied by default");
  assert.equal(permissions.check(view.webContents, "media"), false, "and so is a permission check");
  // IDEMPOTENT, as the operation's own doc says: a second open re-uses the view and re-navigates it.
  assert.deepEqual(open(slot), { ok: true, url: expected });
  assert.equal(views.length, 1, "a second open() must not stack a second view on the window");
  assert.deepEqual(view.webContents.loaded, [expected, expected]);
});

test("place() shows the view at the SPA's bounds, and hides it when the crate calls the slot too small", () => {
  const { slot, views } = boundModule();
  const big = { x: 1, y: 2, width: 800, height: 600 };
  const small = { x: 0, y: 0, width: 10, height: 10 };
  assert.equal(policy.slotTooSmall(big), false, "guard: the crate calls this slot big enough to show a view in");
  assert.equal(policy.slotTooSmall(small), true, "guard: the small slot is under the crate's minimum");
  assert.equal(policy.slotTooSmall(null), true, "guard: an absent slot is too small");
  slot.place(big);
  const view = views[0];
  assert.deepEqual(view.bounds, big, "the bounds are the SPA's CSS pixels, passed through unchanged");
  assert.equal(view.visible, true);
  assert.equal(slot.state().visible, true, "the state the SPA reads must agree with what was placed");
  slot.place(small);
  assert.equal(view.visible, false, "a too-small slot must HIDE the view, never leave it floating over the SPA");
  assert.equal(slot.state().visible, false, "and the state must say so");
  slot.place(big);
  slot.place(null);
  assert.equal(view.visible, false, "a null slot (the SPA clearing its placeholder) hides it the same way");
});

test("navigate() loads the ONE door's answer and records it — the raw string never reaches loadURL", () => {
  const { slot, asked, views } = boundModule();
  const wanted = home();
  assert.deepEqual(slot.navigate(wanted), { ok: true, url: wanted });
  assert.deepEqual(asked, [wanted], "the door must be asked with the RAW string the SPA or the operator handed over");
  assert.deepEqual(views[0].webContents.loaded, [wanted]);
  assert.equal(slot.state().url, wanted, "the state record reports the URL that was loaded");
});

test("navigate() to a refused address loads the door's blank answer, never the raw target", () => {
  const raw = "https://example.com/";
  const decided = policy.dshTarget(raw, urlPolicy.isDshUrl(raw));
  assert.equal(urlPolicy.isDshUrl(raw), false, "guard: the url policy REFUSES this origin (otherwise this case pins nothing)");
  assert.equal(decided, "about:blank", "guard: the door's answer for a refusal is the blank page");
  const { slot, views } = boundModule();
  assert.deepEqual(slot.navigate(raw), { ok: true, url: decided });
  assert.deepEqual(views[0].webContents.loaded, [decided], "the refused target must not be loaded as written");
});

test("a refused popup target is DROPPED without blanking the page, and an admitted one moves the SAME view", () => {
  const { slot, views } = boundModule();
  const view = views.length ? views[0] : null;
  const reading = home();
  slot.navigate(reading);
  const opened = views[0];
  assert.equal(view, null, "guard: no view existed before the first navigate()");
  // A `target=_blank` link is the norm in the harness's own pages (docs, the operator's links). The DSH
  // view is a single pane on ONE origin, so the popup is always denied and — where the door admits the
  // target — the SAME view navigates instead, which is what keeps the click meaningful.
  const admittedTarget = `http://127.0.0.1:${PORT}/docs`;
  const admittedDecided = policy.dshPopupTarget(admittedTarget, urlPolicy.isDshUrl(admittedTarget));
  assert.notEqual(admittedDecided, undefined, "guard: the crate PASSES an admitted target through");
  assert.deepEqual(opened.webContents.openHandler({ url: admittedTarget }), { action: "deny" }, "the popup itself is always denied");
  assert.deepEqual(opened.webContents.loaded, [reading, admittedDecided], "the SAME view navigates to the admitted target");
  // A refused scheme must NOT collapse the page being read to about:blank — the operator loses what
  // they were reading, which is a worse outcome than the dead link.
  const refused = "javascript:alert(1)";
  assert.equal(policy.dshPopupTarget(refused, urlPolicy.isDshUrl(refused)), undefined, "guard: the crate DROPS a refused target");
  assert.deepEqual(opened.webContents.openHandler({ url: refused }), { action: "deny" });
  assert.deepEqual(opened.webContents.loaded, [reading, admittedDecided], "a refused popup must not navigate the view at all");
  assert.equal(opened.webContents.url, admittedDecided, "and it must not blank what the operator is reading");
  // ANOTHER HOST'S FORWARD IS A DOOR TOO (the pane's host switcher): the extra port arrives from the
  // agent's own table, and until it does the same address is refused like any stranger.
  const other = `http://127.0.0.1:${EXTRA_PORT}/`;
  try {
    assert.deepEqual(slot.navigate(other), { ok: true, url: "about:blank" }, "before the table names it, another host's forward is refused");
    urlPolicy.addDshPort(EXTRA_PORT);
    assert.equal(urlPolicy.isDshUrl(other), true, "guard: the crate admits the port the agent's table named");
    assert.deepEqual(slot.navigate(other), { ok: true, url: other }, "once it is a door, the pane can switch to it");
  } finally {
    urlPolicy.clearExtraDshPorts();
  }
});

test("a crashed renderer hides the dead view and pushes `gone` with its reason and exit code", () => {
  const { slot, views, win } = boundModule();
  open(slot);
  slot.place({ x: 0, y: 0, width: 800, height: 600 });
  const view = views[0];
  assert.equal(slot.state().visible, true, "guard: the view was showing when the renderer died");
  view.webContents.emit("render-process-gone", {}, { reason: "crashed", exitCode: 133 });
  assert.equal(view.visible, false, "the dead view is hidden so the SPA's recovery banner can paint in the slot");
  assert.equal(slot.state().visible, false, "and the state must not claim a dead view is showing");
  assert.deepEqual(win.sent, [["embedded-dsh:gone", { reason: "crashed", exitCode: 133 }]],
    "the pane shows 'starting…' forever without this signal — the reason and exit code are what it reports");
});

test("a destroyed window is sent nothing — the view is still hidden, the push is dropped", () => {
  const { slot, views, win } = boundModule();
  open(slot);
  slot.place({ x: 0, y: 0, width: 800, height: 600 });
  win.destroyed = true;
  views[0].webContents.emit("render-process-gone", {}, { reason: "oom", exitCode: 137 });
  assert.deepEqual(win.sent, [], "a window with no renderer cannot receive a push — sending to it is a crash in the handler");
  // The hide is not conditional on the window surviving. This module always did it in this order; the
  // unification made it the SHARED order (the browser's handler used to return early, leaving a dead view
  // on screen), so what used to be one slot's habit is now the slot's rule.
  assert.equal(views[0].visible, false, "the hide is not conditional on the window surviving");
});

test("state() reports exactly url, title and visible — the DSH has NO back/fwd, so no canBack/canFwd", () => {
  const { slot, views, win } = boundModule();
  const expected = home();
  open(slot);
  views[0].webContents.title = "Harness";
  const state = slot.state();
  assert.deepEqual(Object.keys(state).sort(), ["ok", "title", "url", "visible"],
    "the record's KEY SET is the contract: the browser view's `canBack`/`canFwd` are history the DSH pane does not have");
  assert.deepEqual(state, { ok: true, url: expected, title: "Harness", visible: false });
  assert.equal("canBack" in state, false, "a keystroke here would be a history the pane cannot offer");
  assert.equal("canFwd" in state, false);
  assert.equal(win.attached.length, 1, "guard: the view was attached, so the record describes a live one");
  // A DESTROYED webContents is not a live one: the record falls back to blank rather than reporting a
  // dead object's last URL, and `visible` must be false however the slot was placed. THIS IS THE DSH'S OWN
  // `stateUrl` and not the browser's: the browser's address bar must never go blank, so it reports the last
  // url its door decided; this pane's record describes the LIVE view and nothing else.
  slot.place({ x: 0, y: 0, width: 800, height: 600 });
  views[0].webContents.destroyed = true;
  assert.deepEqual(slot.state(), { ok: true, url: "", title: "", visible: false });
});

test("reload() refuses when there is no live view and reloads the live one", () => {
  const { slot, views } = boundModule();
  assert.deepEqual(slot.reload(), { ok: false }, "there is nothing to reload before a view exists");
  open(slot);
  views[0].webContents.destroyed = true;
  assert.deepEqual(slot.reload(), { ok: false }, "a destroyed webContents cannot be reloaded");
  const fresh = open(slot);
  assert.equal(fresh.ok, true, "guard: open() rebuilt the view");
  assert.deepEqual(slot.reload(), { ok: true });
  assert.equal(views[1].webContents.reloaded, 1, "the LIVE view is the one reloaded");
});

test("recover() closes the dead webContents, detaches it, builds a fresh view and reloads the harness home", () => {
  const { slot, views, win } = boundModule();
  const expected = home();
  open(slot);
  const dead = views[0];
  // A `render-process-gone` renderer leaves the webContents OBJECT alive and dead — which is exactly the
  // case the module's own doc names: "a dead webContents never recovers, so re-create the view".
  dead.webContents.emit("render-process-gone", {}, { reason: "crashed", exitCode: 133 });
  assert.equal(dead.webContents.isDestroyed(), false, "guard: the object is alive, the renderer is not");
  assert.deepEqual(slot.recover(), { ok: true });
  assert.equal(dead.webContents.closed, 1, "the dead webContents is CLOSED before the re-create");
  assert.equal(dead.webContents.isDestroyed(), true);
  assert.equal(views.length, 2, "and a full re-create is the only recovery: a second view exists");
  assert.equal(win.attached.includes(dead), false, "the dead view must be removed from the window");
  assert.deepEqual(win.attached, [views[1]], "the fresh view takes its place");
  assert.deepEqual(views[1].webContents.loaded, [expected], "recovery loads the harness home, through the same crate decision");
});

test("recover() still re-creates a view whose webContents is ALREADY destroyed — there is nothing left to close", () => {
  const { slot, views, win } = boundModule();
  const expected = home();
  open(slot);
  const dead = views[0];
  dead.webContents.destroyed = true;
  assert.deepEqual(slot.recover(), { ok: true });
  assert.equal(dead.webContents.closed, 0, "a destroyed webContents is not closed — the guard skips it");
  assert.equal(views.length, 2, "the re-create is unconditional: recovery never re-uses a dead object");
  assert.deepEqual(win.attached, [views[1]], "the fresh view takes the dead one's place");
  assert.deepEqual(views[1].webContents.loaded, [expected]);
});

// ── THE ONE BEHAVIOUR THE UNIFICATION DECIDED ─────────────────────────────────────────────────────────
// `recover()`'s "re-show if the SPA slot is live" block read the visible flag that `ensure()` — which
// recovery had just called to build the fresh view — had already cleared for it. So on the normal path it
// could not run (the one path where it DID was a `close()` throw, where the walk to `ensure()` returned the
// view that was never nulled). The module now reads the placement BEFORE the re-create, so the block runs,
// and it restores the flag so the record agrees with the screen. This case asserted the OLD, unreachable
// behaviour until this change; it was flipped in the same commit rather than left describing a branch that
// no longer exists.
test("recover() RE-SHOWS the fresh view at the slot's last placement — the re-show branch reads it in time", () => {
  const { slot, views } = boundModule();
  const bounds = { x: 1, y: 2, width: 800, height: 600 };
  open(slot);
  slot.place(bounds);
  assert.equal(slot.state().visible, true, "guard: the slot WAS live before the crash");
  views[0].webContents.destroyed = true;
  assert.deepEqual(slot.recover(), { ok: true });
  const fresh = views[1];
  assert.equal(fresh.visible, true, "the fresh view is re-shown at the placement the SPA last reported");
  assert.deepEqual(fresh.bounds, bounds, "and its bounds are re-applied");
  assert.equal(slot.state().visible, true, "so the pane sees a visible view — no second place() is required");
  // AND A SLOT THAT WAS NOT LIVE STAYS HIDDEN: `gone` clears the flag before a recovery driven by the
  // event, and a slot the SPA had hidden must not be resurrected behind its back.
  slot.place(null);
  fresh.webContents.destroyed = true;
  assert.deepEqual(slot.recover(), { ok: true });
  assert.equal(views[2].visible, false, "a slot the SPA had hidden is left hidden");
  assert.equal(views[2].bounds, null, "and no bounds are re-applied to it");
});

test("the window is read through the getter at the moment of use, so a window that appears later is used", () => {
  const shell = boundModule({ withWindow: false });
  const expected = home();
  assert.deepEqual(open(shell.slot), { ok: true, url: expected }, "the view is built even with no window to attach it to");
  assert.equal(shell.views.length, 1);
  assert.equal(shell.views[0].visible, false, "a view with no window is never shown");
  // THE GETTER, AND WHY IT IS NOT A VALUE: the shell builds its window at `app.whenReady()` and nulls it
  // on close, so a snapshot taken at bind time would outlive its window — or precede it, as here.
  const later = shell.newWindow();
  shell.win = later;
  shell.slot.place({ x: 0, y: 0, width: 800, height: 600 });
  assert.deepEqual(later.attached, [shell.views[0]], "the window that arrived later is the one the view attaches to");
  assert.equal(shell.views[0].visible, true, "and place() shows it there");
  assert.equal(shell.slot.state().visible, true);
});

test("place() with no window at all leaves the view built but unplaced — no window to place it in", () => {
  const { slot, views } = boundModule({ withWindow: false });
  slot.place({ x: 0, y: 0, width: 800, height: 600 });
  assert.equal(views.length, 1, "ensure() still builds (and navigates) the view");
  assert.equal(views[0].visible, false, "but there is no window, so nothing is shown");
  assert.equal(views[0].bounds, null, "and no bounds are recorded — the early return is BEFORE the placement");
  assert.deepEqual(slot.state().visible, false);
});
