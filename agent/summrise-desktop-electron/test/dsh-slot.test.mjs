// THE DSH VIEW, EXECUTED — the committed module, a stubbed Electron, the real policy wasm.
//
// WHY THIS FILE EXISTS, AND WHY IT IS THE REASON THE EXTRACTION HAPPENED AT ALL. `src/dsh-view.ts` is
// the shell's SECOND `WebContentsView` (the embedded real-render browser is the first) and it was
// verified by nothing: it lived inside `main.ts`, which no suite in this package can import because
// importing it starts the whole application, so the properties below had no instrument at all. The
// recorded next move is to UNIFY this module with `embedded-view.ts` — a function-by-function
// comparison found the two to be near-copies, and the shared core is to become one parametrised slot —
// and a unification performed before these behaviours were pinned would be a rewrite of two views
// measured against nothing. So the extraction came first, and this file is what it was for.
//
// WHAT IT LOADS, AND WHY THAT FILE. The committed `src/dsh-view.js` — the CommonJS output that
// `src/main.js` requires and the release ships — not the `.ts`. CI already holds the pair together (the
// `electron src freshness` step re-runs `tsc` and `cmp`s the result against the committed `.js`), and
// the artifact worth executing is the one that reaches the operator's device.
//
// HOW IT LOADS IT WITHOUT INSTALLING ELECTRON. A CommonJS module needs four things — `exports`,
// `require`, `module`, `__filename`/`__dirname` — so this file supplies them and evaluates the source
// with its own `require`: the ONE specifier that has to be intercepted is the bare `electron`, and
// everything else (`./summrise_shell_policy`) goes through the real loader rooted at `src/`, so the
// module and this test share ONE instance of the real policy wasm. The module's text is never edited,
// regexed or shimmed.
//
// A FRESH MODULE AND A FRESH STUB PER TEST. The module keeps its view, its visibility and its last
// bounds in module state and binds its host once, so a shared instance would make every test depend on
// the order the others ran in — and the state a test could not reach would be the one that hid a
// defect. Each test therefore gets a new stub AND a new evaluation of the source, the move
// `gateway/wasm/run-cases.mjs` already makes and names at line 173: "a fresh instance is also what a
// new isolate is."
//
// THE SEAM IS BOUND THE WAY `main.ts` BINDS IT, and the four entries are the shell's own: the window is
// a getter (the shell re-creates it and nulls it on close), `targetFor` is `main.ts`'s `dshTargetFor`
// (the url policy's admission composed with the shell policy's serialization), and `admitted`/`base`
// are the url policy's `isDshUrl` and `dshBase` — the two answers its own crate docs say arrive "from
// the host". What this file CANNOT prove is that `main.ts` binds them: that is `test/ipc-door.test.mjs`'s
// source scan, and the two files exist together because neither can see the other's half.
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
const VIEW_JS = path.join(SRC, "dsh-view.js");

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

/** The Electron surface `dsh-view.js` touches, and NOTHING besides it: every method below exists
 *  because the module calls it, so this object is that module's platform boundary read off the module
 *  rather than invented beside it. `views` is every view the module built, in creation order — the DSH
 *  view is re-created on recovery, so the stub records instances instead of a last one. */
function fakeShell({ withWindow = true } = {}) {
  const views = [];
  const win = withWindow ? fakeWindow() : null;

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

  return { electron: { WebContentsView }, win, views, newWindow: () => fakeWindow() };
}

/** The committed module, evaluated against `electron` — a CommonJS module with its one bare specifier
 *  answered by the stub and every other specifier resolved by the real loader, from `src/`. */
function loadDshView(electron) {
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

/** A bound module over a fresh stub, with the four things `main.ts` really binds. `targetFor` IS the
 *  shell's one DSH door (`dshTarget` over the url policy's `isDshUrl`), wrapped so the test can see the
 *  RAW string the module handed it — the same way `embedded-view.test.mjs` wraps `sanitizeBrowserUrl`. */
function boundModule(options) {
  const shell = fakeShell(options);
  const mod = loadDshView(shell.electron);
  const asked = [];
  urlPolicy.setDshPort(PORT);
  mod.dshViewBind({
    win: () => shell.win,
    targetFor: (raw) => { asked.push(raw); return policy.dshTarget(raw, urlPolicy.isDshUrl(raw)); },
    admitted: (raw) => urlPolicy.isDshUrl(raw),
    base: () => urlPolicy.dshBase(),
  });
  return Object.assign(shell, { mod, asked });
}

/** A module over a fresh stub that was NEVER bound — the state `main.ts` is in for the microseconds
 *  between its module load and its `dshViewBind` call. The stub's view list comes back too, because
 *  what the module does with a view it built BEFORE the refusal is half of what is under test. */
function unboundModule() {
  const shell = fakeShell();
  return { ...shell, mod: loadDshView(shell.electron) };
}

/** The address `open()` and `recover()` must load, ASKED OF THE CRATE — and a guard that the port this
 *  file set is the one the crate answers with, so neither case can pass against a hard-coded address. */
function home() {
  urlPolicy.setDshPort(PORT);
  const base = urlPolicy.dshBase();
  assert.equal(base, `http://127.0.0.1:${PORT}`, "guard: setDshPort really moved the crate's DSH base off its default");
  return policy.dshHome(base);
}

test("what happens before dshViewBind: every operation that needs the host throws the loud bind error", () => {
  const { electron } = fakeShell();
  const mod = loadDshView(electron);
  const LOUD = /dshViewBind\(\) was never called/;
  // THE TWO THAT NEVER THROW, AND THAT IS THE RULE: `reload` and `state` read only the view, so with no
  // view they answer exactly as they do when the view is gone — a dead pane is not a crashed shell.
  assert.deepEqual(mod.dshReload(), { ok: false }, "reload with no view is an ordinary refusal, not an error");
  assert.deepEqual(mod.dshState(), { ok: true, url: "", title: "", visible: false });
  // An unbound host would otherwise attach the view to nothing and navigate through a fallback door —
  // the failures a reader would have to reconstruct from a blank pane — so each REFUSES instead.
  assert.throws(() => mod.dshNavigate(home()), LOUD, "navigate must refuse before it can load anything");
  assert.throws(() => mod.dshOpen(), LOUD, "open must refuse rather than build a view attached to nothing");
  assert.throws(() => mod.dshPlace({ x: 0, y: 0, width: 800, height: 600 }), LOUD);
  assert.throws(() => mod.dshRecover(), LOUD);
});

test("what happens before dshViewBind: a refusal part-way through ensure() leaves the module holding the view it built", () => {
  // THE SEAM'S OWN SHAPE, AND IT IS NOT THE ORIGINAL'S. In `main.ts` the window was a `let` that could be
  // null but never threw; the seam's getter REFUSES when unbound, and `dshViewEnsure` reads it AFTER
  // `dshView = view;` — so the loud throw leaves a fully built view in the module's state, which no
  // window ever showed. It is unreachable in the shipped shell (`main.ts` binds at module load, before
  // any `embedded-dsh:*` handler can run) and `embedded-view.ts` has the same ordering, so the
  // unification inherits it from both rather than from either. Pinned because a fix would be a
  // behaviour change and this move is not allowed to be one.
  const { mod, views } = unboundModule();
  assert.throws(() => mod.dshPlace({ x: 0, y: 0, width: 800, height: 600 }), /dshViewBind\(\) was never called/);
  assert.equal(views.length, 1, "the view was built before the refusal — the assignment precedes the getter read");
  assert.deepEqual(mod.dshState(), { ok: true, url: "about:blank", title: "", visible: false },
    "so the module answers about a view that was never shown, and `visible` is still false for it");
  assert.deepEqual(mod.dshReload(), { ok: true }, "and a reload reaches that orphan — there is no live window behind it");
});

test("what happens before dshViewBind: the leaked view's crash handler throws too — its listeners are live, not inert", () => {
  // The other half of the same shape, and the half a reader would not guess: the leaked view has its
  // handlers WIRED, so a `render-process-gone` on it runs code that reads the window through the same
  // refusing getter. The hide happens BEFORE that read, so the visible half of the handler still works
  // and the throw is on the push — the order the "destroyed window" case below pins from the other side.
  const { mod, views } = unboundModule();
  const LOUD = /dshViewBind\(\) was never called/;
  assert.throws(() => mod.dshPlace({ x: 0, y: 0, width: 800, height: 600 }), LOUD);
  assert.equal(views.length, 1, "guard: place() built the view before refusing (open() would refuse at the base, first)");
  assert.throws(
    () => views[0].webContents.emit("render-process-gone", {}, { reason: "crashed", exitCode: 1 }),
    LOUD,
    "the push needs the window, so an unbound module throws from inside the handler",
  );
  assert.equal(views[0].visible, false, "the hide needs nothing from the host, so it ran before the throw");
});

test("the module's seven exports are functions — six main.ts destructures by name, plus the wiring helper", () => {
  const { electron } = fakeShell();
  const mod = loadDshView(electron);
  // The WHOLE surface, not only the names main.ts imports: a RENAMED export is a `TypeError` in a shell
  // that no suite here can start — the failure this package exists to catch before a release.
  // `dshViewEnsure` is deliberately NOT here: nothing outside the module calls it (the three call sites
  // are navigate, place and recover), so it stays private rather than becoming a boundary to maintain.
  assert.deepEqual(
    Object.keys(mod).sort(),
    ["dshNavigate", "dshOpen", "dshPlace", "dshRecover", "dshReload", "dshState", "dshViewBind"],
    "a new export is a new boundary — main.ts, preload.ts and the panel's fixture have to be told about it",
  );
});

test("open() builds the view LAZILY, hidden, with the browser view's security posture, and loads the harness home", () => {
  const { mod, views, win, asked } = boundModule();
  // LAZY: reading the state must not build a view — the SPA asks for state on a page the operator may
  // never open, and a view per page would be a Chromium process per page.
  assert.deepEqual(mod.dshState(), { ok: true, url: "", title: "", visible: false }, "state() before any open");
  assert.equal(views.length, 0, "state() must not create the view");
  const expected = home();
  assert.deepEqual(mod.dshOpen(), { ok: true, url: expected }, "open() answers with the address it loaded");
  assert.equal(views.length, 1, "open() creates the view");
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
  // IDEMPOTENT, as its own doc says: a second open re-uses the view and re-navigates it.
  assert.deepEqual(mod.dshOpen(), { ok: true, url: expected });
  assert.equal(views.length, 1, "a second open() must not stack a second view on the window");
  assert.deepEqual(view.webContents.loaded, [expected, expected]);
});

test("place() shows the view at the SPA's bounds, and hides it when the crate calls the slot too small", () => {
  const { mod, views } = boundModule();
  const big = { x: 1, y: 2, width: 800, height: 600 };
  const small = { x: 0, y: 0, width: 10, height: 10 };
  assert.equal(policy.slotTooSmall(big), false, "guard: the crate calls this slot big enough to show a view in");
  assert.equal(policy.slotTooSmall(small), true, "guard: the small slot is under the crate's minimum");
  assert.equal(policy.slotTooSmall(null), true, "guard: an absent slot is too small");
  mod.dshPlace(big);
  const view = views[0];
  assert.deepEqual(view.bounds, big, "the bounds are the SPA's CSS pixels, passed through unchanged");
  assert.equal(view.visible, true);
  assert.equal(mod.dshState().visible, true, "the state the SPA reads must agree with what was placed");
  mod.dshPlace(small);
  assert.equal(view.visible, false, "a too-small slot must HIDE the view, never leave it floating over the SPA");
  assert.equal(mod.dshState().visible, false, "and the state must say so");
  mod.dshPlace(big);
  mod.dshPlace(null);
  assert.equal(view.visible, false, "a null slot (the SPA clearing its placeholder) hides it the same way");
});

test("navigate() loads the ONE door's answer and records it — the raw string never reaches loadURL", () => {
  const { mod, asked, views } = boundModule();
  const wanted = home();
  assert.deepEqual(mod.dshNavigate(wanted), { ok: true, url: wanted });
  assert.deepEqual(asked, [wanted], "the door must be asked with the RAW string the SPA or the operator handed over");
  assert.deepEqual(views[0].webContents.loaded, [wanted]);
  assert.equal(mod.dshState().url, wanted, "the state record reports the URL that was loaded");
});

test("navigate() to a refused address loads the door's blank answer, never the raw target", () => {
  const raw = "https://example.com/";
  const decided = policy.dshTarget(raw, urlPolicy.isDshUrl(raw));
  assert.equal(urlPolicy.isDshUrl(raw), false, "guard: the url policy REFUSES this origin (otherwise this case pins nothing)");
  assert.equal(decided, "about:blank", "guard: the door's answer for a refusal is the blank page");
  const { mod, views } = boundModule();
  assert.deepEqual(mod.dshNavigate(raw), { ok: true, url: decided });
  assert.deepEqual(views[0].webContents.loaded, [decided], "the refused target must not be loaded as written");
});

test("a refused popup target is DROPPED without blanking the page, and an admitted one moves the SAME view", () => {
  const { mod, views } = boundModule();
  const view = views.length ? views[0] : null;
  const reading = home();
  mod.dshNavigate(reading);
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
    assert.deepEqual(mod.dshNavigate(other), { ok: true, url: "about:blank" }, "before the table names it, another host's forward is refused");
    urlPolicy.addDshPort(EXTRA_PORT);
    assert.equal(urlPolicy.isDshUrl(other), true, "guard: the crate admits the port the agent's table named");
    assert.deepEqual(mod.dshNavigate(other), { ok: true, url: other }, "once it is a door, the pane can switch to it");
  } finally {
    urlPolicy.clearExtraDshPorts();
  }
});

test("a crashed renderer hides the dead view and pushes `gone` with its reason and exit code", () => {
  const { mod, views, win } = boundModule();
  mod.dshOpen();
  mod.dshPlace({ x: 0, y: 0, width: 800, height: 600 });
  const view = views[0];
  assert.equal(mod.dshState().visible, true, "guard: the view was showing when the renderer died");
  view.webContents.emit("render-process-gone", {}, { reason: "crashed", exitCode: 133 });
  assert.equal(view.visible, false, "the dead view is hidden so the SPA's recovery banner can paint in the slot");
  assert.equal(mod.dshState().visible, false, "and the state must not claim a dead view is showing");
  assert.deepEqual(win.sent, [["embedded-dsh:gone", { reason: "crashed", exitCode: 133 }]],
    "the pane shows 'starting…' forever without this signal — the reason and exit code are what it reports");
});

test("a destroyed window is sent nothing — the view is still hidden, the push is dropped", () => {
  const { mod, views, win } = boundModule();
  mod.dshOpen();
  mod.dshPlace({ x: 0, y: 0, width: 800, height: 600 });
  win.destroyed = true;
  views[0].webContents.emit("render-process-gone", {}, { reason: "oom", exitCode: 137 });
  assert.deepEqual(win.sent, [], "a window with no renderer cannot receive a push — sending to it is a crash in the handler");
  // The hide happens FIRST in this module (unlike `embedded-view.ts`, whose early return sits above it),
  // so the dead DSH view is still taken off the screen. That ordering difference is one of the things
  // the unification has to decide rather than absorb.
  assert.equal(views[0].visible, false, "the hide is not conditional on the window surviving");
});

test("state() reports exactly url, title and visible — the DSH has NO back/fwd, so no canBack/canFwd", () => {
  const { mod, views, win } = boundModule();
  const expected = home();
  mod.dshOpen();
  views[0].webContents.title = "Harness";
  const state = mod.dshState();
  assert.deepEqual(Object.keys(state).sort(), ["ok", "title", "url", "visible"],
    "the record's KEY SET is the contract: the browser view's `canBack`/`canFwd` are history the DSH pane does not have");
  assert.deepEqual(state, { ok: true, url: expected, title: "Harness", visible: false });
  assert.equal("canBack" in state, false, "a keystroke here would be a history the pane cannot offer");
  assert.equal("canFwd" in state, false);
  assert.equal(win.attached.length, 1, "guard: the view was attached, so the record describes a live one");
  // A DESTROYED webContents is not a live one: the record falls back to blank rather than reporting a
  // dead object's last URL, and `visible` must be false however the slot was placed.
  mod.dshPlace({ x: 0, y: 0, width: 800, height: 600 });
  views[0].webContents.destroyed = true;
  assert.deepEqual(mod.dshState(), { ok: true, url: "", title: "", visible: false });
});

test("reload() refuses when there is no live view and reloads the live one", () => {
  const { mod, views } = boundModule();
  assert.deepEqual(mod.dshReload(), { ok: false }, "there is nothing to reload before a view exists");
  mod.dshOpen();
  views[0].webContents.destroyed = true;
  assert.deepEqual(mod.dshReload(), { ok: false }, "a destroyed webContents cannot be reloaded");
  const fresh = mod.dshOpen();
  assert.equal(fresh.ok, true, "guard: open() rebuilt the view");
  assert.deepEqual(mod.dshReload(), { ok: true });
  assert.equal(views[1].webContents.reloaded, 1, "the LIVE view is the one reloaded");
});

test("recover() closes the dead webContents, detaches it, builds a fresh view and reloads the harness home", () => {
  const { mod, views, win } = boundModule();
  const expected = home();
  mod.dshOpen();
  const dead = views[0];
  // A `render-process-gone` renderer leaves the webContents OBJECT alive and dead — which is exactly the
  // case the module's own doc names: "a dead webContents never recovers, so re-create the view".
  dead.webContents.emit("render-process-gone", {}, { reason: "crashed", exitCode: 133 });
  assert.equal(dead.webContents.isDestroyed(), false, "guard: the object is alive, the renderer is not");
  assert.deepEqual(mod.dshRecover(), { ok: true });
  assert.equal(dead.webContents.closed, 1, "the dead webContents is CLOSED before the re-create");
  assert.equal(dead.webContents.isDestroyed(), true);
  assert.equal(views.length, 2, "and a full re-create is the only recovery: a second view exists");
  assert.equal(win.attached.includes(dead), false, "the dead view must be removed from the window");
  assert.deepEqual(win.attached, [views[1]], "the fresh view takes its place");
  assert.deepEqual(views[1].webContents.loaded, [expected], "recovery loads the harness home, through the same crate decision");
});

test("recover() still re-creates a view whose webContents is ALREADY destroyed — there is nothing left to close", () => {
  const { mod, views, win } = boundModule();
  const expected = home();
  mod.dshOpen();
  const dead = views[0];
  dead.webContents.destroyed = true;
  assert.deepEqual(mod.dshRecover(), { ok: true });
  assert.equal(dead.webContents.closed, 0, "a destroyed webContents is not closed — the guard skips it");
  assert.equal(views.length, 2, "the re-create is unconditional: recovery never re-uses a dead object");
  assert.deepEqual(win.attached, [views[1]], "the fresh view takes the dead one's place");
  assert.deepEqual(views[1].webContents.loaded, [expected]);
});

// ── PRE-EXISTING, PINNED AND NOT FIXED ────────────────────────────────────────────────────────────────
// `recover()`'s "re-show if the SPA slot is live" block reads `dshVisible`, and `dshViewEnsure()` — which
// recovery has just called to build the fresh view — has already set that flag to FALSE for it. So on the
// normal path the block cannot run: a recovered view is left hidden with `bounds = null`, and the SPA's
// next `place()` is what shows it. (The one path where it DOES run is the `close()` throw below the
// comment, where the walk to `ensure()` returns the view that was never nulled — which is why the block
// is unreachable-in-practice rather than dead code.) This is the same shape `embedded-view.ts` was found
// to have; a no-behaviour-change move that quietly fixed it here would be two changes wearing one commit.
test("PRE-EXISTING: recover() leaves the fresh view hidden — its re-show branch reads the flag ensure() just cleared", () => {
  const { mod, views } = boundModule();
  const bounds = { x: 1, y: 2, width: 800, height: 600 };
  mod.dshOpen();
  mod.dshPlace(bounds);
  assert.equal(mod.dshState().visible, true, "guard: the slot WAS live before the crash");
  views[0].webContents.destroyed = true;
  assert.deepEqual(mod.dshRecover(), { ok: true });
  const fresh = views[1];
  assert.equal(fresh.visible, false, "PRE-EXISTING: the recovered view is left HIDDEN, not re-shown at its old bounds");
  assert.equal(fresh.bounds, null, "PRE-EXISTING: and its bounds were never re-applied");
  assert.equal(mod.dshState().visible, false, "so the SPA sees a hidden view and places it again");
});

test("the window is read through the getter at the moment of use, so a window that appears later is used", () => {
  const shell = boundModule({ withWindow: false });
  const expected = home();
  assert.deepEqual(shell.mod.dshOpen(), { ok: true, url: expected }, "the view is built even with no window to attach it to");
  assert.equal(shell.views.length, 1);
  assert.equal(shell.views[0].visible, false, "a view with no window is never shown");
  // THE GETTER, AND WHY IT IS NOT A VALUE: the shell builds its window at `app.whenReady()` and nulls it
  // on close, so a snapshot taken at bind time would outlive its window — or precede it, as here.
  const later = shell.newWindow();
  shell.win = later;
  shell.mod.dshPlace({ x: 0, y: 0, width: 800, height: 600 });
  assert.deepEqual(later.attached, [shell.views[0]], "the window that arrived later is the one the view attaches to");
  assert.equal(shell.views[0].visible, true, "and place() shows it there");
  assert.equal(shell.mod.dshState().visible, true);
});

test("place() with no window at all leaves the view built but unplaced — no window to place it in", () => {
  const { mod, views } = boundModule({ withWindow: false });
  mod.dshPlace({ x: 0, y: 0, width: 800, height: 600 });
  assert.equal(views.length, 1, "ensure() still builds (and navigates) the view");
  assert.equal(views[0].visible, false, "but there is no window, so nothing is shown");
  assert.equal(views[0].bounds, null, "and no bounds are recorded — the early return is BEFORE the placement");
  assert.deepEqual(mod.dshState().visible, false);
});
