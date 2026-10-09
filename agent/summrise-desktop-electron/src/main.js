"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
// Summrise Desktop — Electron shell over the local summrise-agent service (TS).
// UI = agent /desktop/ route (panel-react SPA); agent = child process.
//
// stage-m architecture: agent lifecycle lives in RUST (the agent is managed
// by the SummriseAgent scheduled task; a second agent instance exits immediately
// via SUMMRISE_NO_PAUSE on bind failure — no orphan processes). This shell only:
//   1. probes the agent's configured bind port (default 127.0.0.1:18080)
//      for health (follows config.yaml server.port)
//   2. loads the SPA when the agent is up; shows a "start agent" action
//      otherwise (schtasks /run — the ONLY sanctioned spawn path)
//   3. window / tray / native menu / CDP exposure / browser sessions
// No spawn() of summrise-agent.exe from JS — that was the d1 Chrome-OOM root
// cause (a bind-failed child wedged on "Press Enter to exit" forever).
const electron_1 = require("electron");
// IPC audit #6: execSync/ChildProcess were vestigial imports from the
// pre-async-spawn era (round-117 removed the call sites) — dropped so
// eslint/tsc keep the surface honest. The last one to go was the empty
// `import type {} from "child_process"` that stood here: it imported
// NOTHING (the schtasks runner reaches the module through require() with its
// own `as typeof import(...)` type), and a type-only import of a module the
// shell does not otherwise import is a "dependency" no reader can act on.
const path = __importStar(require("path"));
const fs = __importStar(require("fs"));
const http = __importStar(require("http"));
// THE POLICY IS RUST, IN TWO CRATES, AND THIS FILE IS THE HOST THAT CALLS IT.
//
// `agent/summrise-url-policy` (landing 6a) owns the URL/origin/certificate decisions; `cargo test -p
// summrise-url-policy` pins them. `agent/summrise-shell-policy` (landing 6b) owns everything else this
// file used to decide: the device token's line pattern, the auth header, the port precedence, the icon
// per platform, the native menu table, the browser-session plan and both embedded views' doors, the
// loopback control server's router, the schtasks arguments, the watchdog, the tripwire, the CDP
// self-check and every string the tray shows. `cargo test -p summrise-shell-policy` pins those.
//
// BOTH are built to wasm with `wasm-pack --target nodejs` and required here; the glue compiles each
// module SYNCHRONOUSLY, so the first decision may run immediately — which matters, because the first
// thing this file does is pin the agent's port. The glues and the modules are committed and shipped
// (agent/summrise-agent-npm/required-in-tgz.txt names all four files, and `summrise update` stages them).
//
// **THE TWO CRATES DO NOT DEPEND ON EACH OTHER, AND THAT IS A MEASURED CONSTRAINT RATHER THAN A STYLE.**
// wasm-bindgen exports every `#[wasm_bindgen]` item in the whole crate GRAPH, so making the shell-policy
// crate depend on the url-policy one made its glue re-export all 18 url-policy names — and each glue
// compiles its OWN `.wasm`, so url-policy's `thread_local!` port state would exist TWICE.
// `setAgentPort(7740)` through one module would leave the other at 18080: a custom-port install where the
// DSH door admits the wrong origin and the tripwire snaps the panel back. So where a decision below
// composes a url-policy predicate it passes that predicate's ANSWER in — `isDshUrl(raw)`,
// `isDesktopSpaUrl(url)`, `isBaseOrigin(url)`, `parseAgentPort(raw)`, `sanitizeBrowserUrl(raw)` — and
// this file, which holds exactly one instance of each glue, evaluates it.
const summrise_url_policy_1 = require("./summrise_url_policy");
const summrise_shell_policy_1 = require("./summrise_shell_policy");
const K = JSON.parse((0, summrise_shell_policy_1.shellConstants)());
// The handful the shell reads often enough that `K.` at every use site would be noise. They are still the
// table's values — destructuring a copy of the parse, not a second declaration.
const { AGENT_TASK, SCHTASKS_TIMEOUT_MS, AGENT_PROBE_DEFAULT_MS, AGENT_PROBE_BOOT_MS, AGENT_PROBE_TRAY_MS, CONTROL_PROBE_MS, STATUS_FETCH_MS, HARNESS_FETCH_MS, CDP_CHECK_MS, MENU_FLUSH_MS, TRAY_POLL_MS, RETRY_START_MS, TRIPWIRE_BACKOFF_MS, } = K;
// IPC audit #3: /api/status is TOKEN-GATED (same fact the watchdog fix cites);
// credential-less fetches got 401 -> version title + tray vitals were DEAD on
// every configured device. The shell runs as the interactive admin, and the
// config is now ACL-restricted to SYSTEM+Administrators (1.2.226) — reading
// the local device_token from it is exactly the trust that grants.
// Layout v2 (ADR 0008): the shell lives at
// <install>\components\summrise-desktop-electron\src\, so the install root is
// THREE levels up from here (was two before components\ existed). All
// install-root-relative paths derive from this one const.
const INSTALL_ROOT = path.join(__dirname, "..", "..", "..");
let _tokenCache = { at: 0, tok: null };
function agentToken() {
    if ((0, summrise_shell_policy_1.tokenCacheFresh)(_tokenCache.at, Date.now()))
        return _tokenCache.tok;
    let raw;
    try {
        raw = fs.readFileSync(path.join(INSTALL_ROOT, "etc", "config.yaml"), "utf8");
    }
    catch { /* no local config — vitals stay hidden, same as before */ }
    // THE PATTERN IS THE POLICY CRATE'S, and it is deliberately NARROWER than the two other parsers of
    // this line: the CLI's (bin/summrise.js, `[A-Za-z0-9._-]+`) and the agent's Rust recovery both accept
    // more than lowercase hex. Measured against a live device on 2026-09-24 — the token is 64 hex
    // characters with no space before the colon, so all three agree TODAY, and hex is what the worker
    // issues. The failure mode if that ever stops being true is SILENT: no credential, a 401, a title that
    // stays "Summrise" and vitals that stay blank. `cargo test -p summrise-shell-policy` pins which side of
    // that divergence the pattern is on; WIDEN IT THERE before debugging this.
    const tok = (0, summrise_shell_policy_1.deviceToken)(raw) ?? null;
    _tokenCache = { at: Date.now(), tok };
    return tok;
}
function authHeaders() {
    const value = (0, summrise_shell_policy_1.authorization)(agentToken());
    return value ? { authorization: value } : {};
}
// Agent bind port (custom-port installs): explicit SUMMRISE_AGENT_PORT env
// first, then the agent's config.yaml server.port next to the install dir
// (same file agentToken() reads — sync fs, same best-effort discipline),
// else the canonical 18080. Resolved once at boot before any probe/window;
// the Rust policy's predicates follow via setAgentPort (which ignores an
// invalid port rather than resetting to the default).
function bootAgentPort() {
    // The ORDER (env, then the device's own config, then the canonical port) is the policy crate's; the
    // YAML walk that produces the middle answer is the URL policy's, which is why it is evaluated HERE and
    // passed in. Both reads are best-effort and both fall through to the next source.
    let fromConfig;
    try {
        fromConfig = (0, summrise_url_policy_1.parseAgentPort)(fs.readFileSync(path.join(INSTALL_ROOT, "etc", "config.yaml"), "utf8")) ?? undefined;
    }
    catch { /* no local config — default below */ }
    return (0, summrise_shell_policy_1.resolveAgentPort)(process.env.SUMMRISE_AGENT_PORT, fromConfig);
}
// Remote-verifiable icon facts (GET /api/shell/icon-status on the 9444
// loopback control server): file blind-flying on icon issues ended here —
// every surface records what it resolved so the console can read it back.
const iconReport = { platform: process.platform };
function statSize(p) {
    try {
        return fs.existsSync(p) ? fs.statSync(p).size : -1;
    }
    catch {
        return -1;
    }
}
// Brand icon for every native surface: resolve the icon path (existsSync
// guarded) and record the outcome for /api/shell/icon-status. appIcon and
// windowIcon used to each inline this.
function resolveIcon(name, reportKey) {
    const p = path.join(__dirname, "..", name);
    let out = "";
    try {
        out = fs.existsSync(p) ? p : "";
    }
    catch {
        out = "";
    }
    iconReport[reportKey] = { path: p, size: statSize(p), used: out !== "" };
    return out;
}
// Brand icon for every native surface. Tray on Windows needs .ico;
// BrowserWindow takes .png on ALL platforms (Skia-decodes reliably —
// Chromium's ICO parser has choked on PNG-compressed 256px entries,
// silently falling back to the stock electron.exe icon, device-caught).
// Empty string when absent — callers fall back to Electron defaults.
function appIcon() {
    // WHICH FILE is the policy crate's decision (`icon.ico` for the Windows tray, `icon.png` everywhere
    // else); resolving and reporting it is this file's.
    return resolveIcon((0, summrise_shell_policy_1.trayIconName)(process.platform), "tray");
}
function windowIcon() {
    return resolveIcon((0, summrise_shell_policy_1.windowIconName)(), "window");
}
// Native-decode probe: file-exists is NOT proof Electron can use the
// image (a corrupt/undecodable file falls back silently). Report what
// nativeImage itself says, so /api/shell/icon-status is ground truth.
function probeImage(p) {
    try {
        if (!p)
            return { empty: true, reason: "no-path" };
        const img = electron_1.nativeImage.createFromPath(p);
        const sz = img.getSize();
        return { empty: img.isEmpty(), width: sz.width, height: sz.height };
    }
    catch (e) {
        return { empty: true, reason: String(e).slice(0, 120) };
    }
}
// IPC audit #2: preload runs in EVERY frame; will-navigate never gated
// iframes. Handlers must reject anything not sourced from the pinned panel.
function frameOk(e) {
    return (0, summrise_url_policy_1.frameUrlOk)(e.senderFrame?.url || "");
}
// THE ONE REFUSAL a forbidden frame gets. It used to have TWO shapes — seven
// handlers answered this one and the rest answered a bare `{ ok: false }`,
// which the SPA cannot tell apart from a dead view (`j?.ok` is falsy either
// way, and only this shape carries the reason). The preload's `invoke` callers
// were written against THIS shape, so every handler answers it now.
// THE VALUE IS THE POLICY CRATE'S, so the one refusal has one owner. `Object.freeze` is kept here
// because it is this file's guarantee to itself, not a decision about what the SPA reads.
const FORBIDDEN_FRAME = Object.freeze(JSON.parse((0, summrise_shell_policy_1.forbiddenFrame)()));
// THE ONE IPC DOOR: `ipcMain.handle` is called HERE and nowhere else, so the
// frame check above is applied ONCE per channel and no handler can forget it.
// A fourteenth handler that re-checked (or skipped) the frame was SILENT:
// main.ts imports electron, so no node suite can import it and notice — the
// pin is a SOURCE check instead (test/ipc-door.test.mjs counts the call sites).
//
// Handlers get the invoke ARGUMENTS only: every one of them used the event for
// nothing but the frame check. The refusal is decided before the handler runs,
// so a forbidden frame never reaches the implementation (no schtasks spawn, no
// browser window, no loadURL).
function ipcHandle(channel, fn) {
    electron_1.ipcMain.handle(channel, (e, ...args) => {
        if (!frameOk(e))
            return FORBIDDEN_FRAME;
        return fn(...args);
    });
}
// P1: CDP port for AI (playwright) to drive Summrise's own pages — the SAME
// Electron window the user watches. Summrise's playwright-mcp connects via
// connectOverCDP("http://127.0.0.1:9333") and drives this window's pages.
// P1b: CTRL_PORT is the fallback local control endpoint for browser sessions
// (used when the SPA runs in a plain browser, not under the Electron preload IPC).
// BOTH NUMBERS ARE THE POLICY CRATE'S (`K.CDP_PORT`, `K.CTRL_PORT`) and NOTHING about when or whether
// they are opened moved with them: the `remote-debugging-port` switch below is still the only thing that
// opens 9333, and 9333 is a product contract — `agent/tests/mcp_autoselect_integration.rs` binds it, the
// CLI probes it, and the design sweep's attached mode depends on it.
let win = null;
let tray = null;
/** Whether the hide-to-tray notification has been shown (once per launch). */
let hideNotified = false;
const browserSessions = new Map();
/** Initial target per browser window — reuse matching uses this instead of
 *  webContents.getURL() (which lags during navigation; stage-n). */
const browserTargets = new Map();
/** Cap on concurrent browser-session windows — an AI loop opening sessions
 *  repeatedly must not pile up windows on the desktop (stage-n). The number is `K.MAX_BROWSER_WINDOWS`;
 *  the PLAN that applies it (reuse by initial target, then evict the oldest) is the policy crate's. */
// stage-m: SINGLE-INSTANCE LOCK — a second Summrise window exits immediately.
// (The agent itself also enforces single-instance via bind-failure exit.)
const gotTheLock = electron_1.app.requestSingleInstanceLock();
if (!gotTheLock) {
    electron_1.app.quit();
}
else {
    electron_1.app.on("second-instance", () => { focusMain(); });
}
// CDP must be enabled before app ready — pass it through Chromium switches.
electron_1.app.commandLine.appendSwitch("remote-debugging-port", String(K.CDP_PORT));
// Lab-device self-signed certs (OpenWrt-style ONT defaults): bypass cert
// errors ONLY on private-network hosts — the public internet keeps full
// validation. Registered before ready so no navigation can race it.
electron_1.app.on("certificate-error", (event, _webContents, url, _error, _certificate, callback) => {
    if ((0, summrise_url_policy_1.certBypassAllowed)(String(url || ""))) {
        event.preventDefault();
        callback(true);
        return;
    }
    callback(false);
});
// round-274 (device-caught): when the window is hidden (hide-to-tray /
// SYSTEM-session background) Chromium flips the page to visibilityState
// "hidden" and STOPS requestAnimationFrame — xterm's rAF-driven DOM
// renderer then never paints, so every terminal goes blank while the AI
// keeps operating. backgroundThrottling:false alone does NOT restore rAF
// for hidden pages; these renderer-level switches do:
//   disable-renderer-backgrounding        — don't pause rAF/timers when the
//                                           renderer is backgrounded
//   disable-backgrounding-occluded-windows — same for occluded windows
electron_1.app.commandLine.appendSwitch("disable-renderer-backgrounding");
electron_1.app.commandLine.appendSwitch("disable-backgrounding-occluded-windows");
// --- Menu command bridge: SPA ⇄ native menu ---
// stage-n: commands issued before the SPA registered its summrise-menu listener
// (preload bridge + React useEffect) are silently dropped by webContents.send.
// Queue them and flush shortly after did-finish-load; the SPA's listener is
// registered within a second of load completing (token connect + effect).
let menuQueue = []; // null = SPA confirmed ready, send directly
let menuFlushTimer = null;
/** Deliver one menu command to the SPA (win validated by callers). */
function emitMenu(cmd) {
    // THE CALLERS VALIDATE `win` — TypeScript cannot see that, so `npm run build`
    // in this directory has been RED on it for as long as the line has existed.
    // Nothing noticed because CI compiles this tree with `--noCheck` (matching the
    // release flow) and this package's own `npm test`/`npm run build` are run by
    // NOBODY — there is no CI step with this working directory. Guarding here says
    // the same thing the comment did, in a form the compiler accepts.
    if (!win || win.isDestroyed())
        return;
    win.webContents.send("summrise-menu", cmd);
}
function sendMenu(cmd) {
    if (win && !win.isDestroyed()) {
        if (menuQueue) {
            menuQueue.push(cmd); // SPA not yet confirmed ready — queue
        }
        else {
            emitMenu(cmd);
        }
    }
}
function flushMenuQueue() {
    if (menuFlushTimer) {
        clearTimeout(menuFlushTimer);
        menuFlushTimer = null;
    }
    if (!win || win.isDestroyed())
        return;
    const q = menuQueue;
    menuQueue = null; // drain: subsequent sends go straight out
    if (q) {
        for (const cmd of q) {
            emitMenu(cmd);
        }
    }
}
function focusMain() {
    if (!win || win.isDestroyed())
        return;
    if (win.isMinimized())
        win.restore();
    win.show();
    win.focus();
}
/** Build the native application menu.
 *
 *  THE TABLE IS THE POLICY CRATE'S — every label, every accelerator and every command id — because the
 *  command ids are the contract with the SPA's own `summrise-menu` listener and the accelerators are the
 *  second half of it (the SPA handles the same chords from its own keydown map in a plain browser). A
 *  renamed id or a dropped row breaks the desktop app and nothing else. `Menu.buildFromTemplate` and the
 *  Electron `role` strings are this file's. */
function buildMenu() {
    const template = JSON.parse((0, summrise_shell_policy_1.appMenuJson)(process.platform, electron_1.app.name)).map((section) => ({
        label: section.label,
        submenu: section.items.map((item) => {
            if (item.separator)
                return { type: "separator" };
            const base = {};
            if (item.label)
                base.label = item.label;
            if (item.accelerator)
                base.accelerator = item.accelerator;
            // THREE KINDS OF ROW, and the crate's table says which is which: a command the SPA dispatches, the
            // ONE host method (Reload), and an Electron role the menu system implements itself.
            if (item.command)
                return { ...base, click: () => sendMenu(item.command) };
            if (item.reload)
                return { ...base, click: () => { if (win)
                        win.webContents.reload(); } };
            return { ...base, role: item.role };
        }),
    }));
    return electron_1.Menu.buildFromTemplate(template);
}
// --- Browser-session control: shared core (used by both IPC and HTTP) ---
/** THE ONE LOAD DOOR (stage-n hardening): every raw URL this shell is asked to
 *  load — a browser-session window, the embedded real-render view, a
 *  window.open() from a page inside it — is decided HERE and nowhere else.
 *
 *  Only http/https/about:blank survive; file:// and every other scheme
 *  collapse to "about:blank", because a loadable file:// would hand the AI a
 *  local-file read primitive via the shared CDP endpoint (:9333).
 *
 *  The DECISION itself is the Rust policy's sanitizeBrowserUrl() — pure, and
 *  unit-tested in agent/summrise-url-policy (src/tests.rs) because main.ts
 *  imports electron and no node suite can import it. This function is that predicate's ONE call site
 *  in the shell (test/ipc-door.test.mjs pins the count), so a fourth load site
 *  cannot quietly reach for a weaker policy of its own.
 *
 *  Callers keep their own about:blank semantics: browserOpen() opens a blank
 *  window, embeddedNavigate() loads it, and the embedded view's window-open
 *  handler REFUSES it (an external link must not blank the page being read). */
function loadTarget(raw) {
    return (0, summrise_url_policy_1.sanitizeBrowserUrl)(raw);
}
/** Open a browser-session window on a decided load target (loadTarget() is the
 *  ONE load door — the scheme policy lives there). */
function browserOpen(url) {
    const target = loadTarget(url);
    // stage-n: reuse an existing window on the same URL instead of stacking duplicates (AI-driven
    // browsing opens/closes sessions repeatedly). Match against the window's INITIAL target (stored at
    // open) — webContents.getURL() lags during navigation (returns about:blank while loading), so a
    // same-URL reopen right after the first open would otherwise miss the reuse and stack a duplicate.
    //
    // THE PLAN IS THE POLICY CRATE'S, in one call: which window to reuse (first non-destroyed match, in
    // INSERTION order), and — when the cap is reached — which one to evict (the OLDEST, not the least
    // recently used). This file only reads the session table and applies the answer. The `destroyed` flag
    // travels with the row because a destroyed window is skipped for reuse while still COUNTING against
    // the cap, which is what `browserSessions.size` did.
    const plan = JSON.parse((0, summrise_shell_policy_1.planBrowserOpen)(JSON.stringify([...browserSessions].map(([id, bw]) => ({ id, target: browserTargets.get(id) ?? "", destroyed: bw.isDestroyed() }))), target, K.MAX_BROWSER_WINDOWS));
    if (plan.reuse) {
        const existing = browserSessions.get(plan.reuse);
        if (existing) {
            if (existing.isMinimized())
                existing.restore();
            existing.show();
            existing.focus();
            return { ok: true, id: plan.reuse, url: target, cdp: (0, summrise_shell_policy_1.cdpEndpoint)(K.CDP_PORT) };
        }
    }
    if (plan.evict)
        browserClose(plan.evict);
    const id = (0, summrise_shell_policy_1.browserId)(Date.now());
    const bw = new electron_1.BrowserWindow({
        width: 1100, height: 750, title: `Summrise Browser — ${target}`,
        ...(windowIcon() ? { icon: windowIcon() } : {}),
        // review #6/#7: these windows load ARBITRARY internet pages. contextIsolation
        // on + nodeIntegration off + NO preload = zero Node/bridge surface (the
        // default; made explicit). Popups are denied (they would otherwise spawn
        // unmanaged windows outside the MAX_BROWSER_WINDOWS cap).
        webPreferences: { contextIsolation: true, nodeIntegration: false, sandbox: true },
    });
    bw.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
    bw.loadURL(target);
    bw.on("closed", () => { browserSessions.delete(id); browserTargets.delete(id); });
    // stage-n: the window title starts as "Summrise Browser — <target>" but a
    // site's own <title> is more useful after load (and after in-page
    // navigation); follow it so window managers/taskbar show the real page.
    bw.webContents.on("page-title-updated", (_e, title) => {
        if (!bw.isDestroyed() && title)
            bw.setTitle(`Summrise Browser — ${title}`);
    });
    browserSessions.set(id, bw);
    browserTargets.set(id, target);
    return { ok: true, id, url: target, cdp: (0, summrise_shell_policy_1.cdpEndpoint)(K.CDP_PORT) };
}
function browserClose(id) {
    const bw = browserSessions.get(id || "");
    // review #g: close() is vetoable by a page's beforeunload — an arbitrary
    // remote page could refuse eviction, so the MAX_BROWSER_WINDOWS cap never
    // freed and browserOpen stalled. destroy() is unconditional (these are
    // disposable, headless-driven windows).
    if (bw) {
        bw.destroy();
        browserSessions.delete(id || "");
        browserTargets.delete(id || "");
    }
    return { ok: true };
}
function browserList() {
    const list = [...browserSessions.entries()].map(([id, bw]) => ({ id, url: bw.webContents.getURL() }));
    return { ok: true, sessions: list, cdp: (0, summrise_shell_policy_1.cdpEndpoint)(K.CDP_PORT) };
}
ipcHandle("browser-session:open", (url) => browserOpen(url));
ipcHandle("browser-session:close", (id) => browserClose(id));
ipcHandle("browser-session:list", () => browserList());
// ── Embedded real-render browser view (round-246) ──────────────────────────
// The SPA's Browser page used to show the BRIDGE's headless-chromium as a
// JPEG screencast (lossy q60-92 frames over a websocket) — never as sharp as
// a real browser, and a SECOND browser instance alongside the desktop shell.
// In the Electron shell we replace it with a REAL WebContentsView embedded
// over the SPA's browser placeholder: GPU-composited, vector text, directly
// interactive. Its webContents is a first-class CDP target on the SAME
// :9333 endpoint, so AI (playwright connectOverCDP) drives EXACTLY the page
// the user sees — one browser, zero JPEG.
//
// Security posture mirrors the browser-session windows: the view loads
// arbitrary internet pages, so contextIsolation on, nodeIntegration off, NO
// preload, sandbox on, popups denied, permissions denied by default.
let embeddedView = null;
let embeddedVisible = false;
let embeddedUrl = "about:blank";
// round-256: last placed bounds — kept so a renderer-crash recovery can
// re-show the fresh view at the same spot without waiting for the SPA.
let embeddedBounds = null;
function embeddedViewEnsure() {
    if (embeddedView && !embeddedView.webContents.isDestroyed())
        return embeddedView;
    const view = new electron_1.WebContentsView({
        webPreferences: {
            contextIsolation: true,
            nodeIntegration: false,
            sandbox: true,
            // round-246 (review #6/#7 parity): no preload = zero Node surface.
        },
    });
    view.setVisible(false);
    embeddedVisible = false;
    // round-248 (user report "浏览器超链接跳转不了"): `target=_blank` links
    // (the norm on real sites — Baidu's every external link opens a new
    // window) were DENIED, so clicking them did nothing. The embedded view is
    // a single-tab browser: intercept window.open and navigate the SAME view
    // to the requested URL instead (the address bar follows via the nav
    // events). The scheme allow-list is loadTarget()'s (the ONE load door):
    // anything it refuses (javascript:, file:, chrome:) must NOT blank the page
    // the user is reading, so a refused target is dropped here instead.
    view.webContents.setWindowOpenHandler(({ url }) => {
        // THE DOOR DECIDES, THIS FILE APPLIES: `loadTarget` is the ONE load door (the URL policy's), and
        // the DECISION to DROP a refused target rather than navigate to the blank page is the policy
        // crate's — a refused `javascript:`/`file:`/`chrome:` link must not erase what the operator is
        // reading, which is why a refusal is `undefined` here and not `"about:blank"`.
        const decided = (0, summrise_shell_policy_1.embeddedPopupTarget)(loadTarget(url));
        if (decided !== undefined) {
            embeddedNavigate(decided);
        }
        return { action: "deny" };
    });
    // round-248: same-view navigation for window.name/target=_top style links
    // that Chromium routes as renderer-initiated top navigations is already
    // handled natively; this handler only covers the window.open path above.
    // round-247: real-navigation events → SPA (URL/title/history tracking).
    embeddedWireNavEvents(view);
    try {
        electron_1.session.defaultSession.setPermissionRequestHandler((_wc, _perm, cb) => cb(false));
        electron_1.session.defaultSession.setPermissionCheckHandler(() => false);
    }
    catch { /* non-fatal (already set at app ready) */ }
    embeddedView = view;
    // Attach to the main window's content view once the window exists. The
    // view is added hidden and only shown when the SPA's Browser page is
    // active and reports its placeholder bounds.
    if (win)
        win.contentView.addChildView(view);
    return view;
}
/** Place the embedded view over the SPA's browser placeholder (CSS px in the
 *  window's content coordinate space). Empty bounds hide it. */
function embeddedViewPlace(bounds) {
    const view = embeddedViewEnsure();
    if (!win)
        return;
    // `K.MIN_SLOT_PX` AND ITS COMPARISON ARE ONE FUNCTION NOW: this view and the DSH view each spelled
    // `bounds.width < 50 || bounds.height < 50` BEFORE the port, which is two copies of one policy — the
    // place where the browser view and the DSH view would come to disagree about what "showing" means.
    if ((0, summrise_shell_policy_1.slotTooSmall)(bounds)) {
        view.setVisible(false);
        embeddedVisible = false;
        return;
    }
    embeddedBounds = bounds;
    view.setBounds(bounds);
    view.setVisible(true);
    embeddedVisible = true;
    // Keep it above the SPA content (the SPA placeholder is an empty div).
    win.contentView.addChildView(view);
}
function embeddedNavigate(raw) {
    // Every raw URL the SPA/AI hands the embedded view is decided by the ONE
    // load door before it reaches loadURL.
    const url = loadTarget(raw);
    const view = embeddedViewEnsure();
    embeddedUrl = url;
    view.webContents.loadURL(url).catch(() => { });
    return { ok: true, url };
}
/** round-247: nav controls on the embedded view (the SPA's toolbar mirrors a
 *  real browser: back/fwd/reload operate the actual webContents history). */
function embeddedGo(delta) {
    const view = embeddedView;
    if (!view || view.webContents.isDestroyed())
        return { ok: false };
    try {
        if ((0, summrise_shell_policy_1.goBackwards)(delta))
            view.webContents.goBack();
        else
            view.webContents.goForward();
        return { ok: true };
    }
    catch {
        return { ok: false };
    }
}
function embeddedReload() {
    const view = embeddedView;
    if (!view || view.webContents.isDestroyed())
        return { ok: false };
    try {
        view.webContents.reload();
        return { ok: true };
    }
    catch {
        return { ok: false };
    }
}
function embeddedState() {
    const view = embeddedView;
    const wc = view && !view.webContents.isDestroyed() ? view.webContents : null;
    return {
        ok: true,
        // The FALLBACK is the policy crate's: a live `getURL()` that answers "" (not yet navigated) and no
        // live contents at all are the same answer, and it is the last URL this view was sent to.
        url: (0, summrise_shell_policy_1.shownUrl)(wc ? wc.getURL() : undefined, embeddedUrl),
        canBack: !!wc && wc.navigationHistory.canGoBack(),
        canFwd: !!wc && wc.navigationHistory.canGoForward(),
        title: wc ? wc.getTitle() : "",
        visible: (0, summrise_shell_policy_1.viewVisible)(embeddedVisible, !!wc),
    };
}
/** round-247: push real navigation state (URL + title + history) to the SPA
 *  so its address bar and back/fwd buttons track the ACTUAL embedded page —
 *  event-driven (fires on navigation, no polling). */
function embeddedWireNavEvents(view) {
    const wc = view.webContents;
    const push = () => {
        if (!win || win.isDestroyed())
            return;
        const s = embeddedState();
        win.webContents.send("embedded-browser:nav", { url: s.url, canBack: s.canBack, canFwd: s.canFwd, title: s.title });
    };
    wc.on("did-navigate", push);
    wc.on("did-navigate-in-page", push);
    wc.on("page-title-updated", push);
    // round-256: the embedded view's RENDERER can crash (process-gone) or hang
    // mid-load (did-fail-load). Push a `gone` event so the SPA shows a recovery
    // state instead of "Starting…" forever; the SPA then offers a reload.
    wc.on("render-process-gone", (_e, details) => {
        if (!win || win.isDestroyed())
            return;
        // Hide the dead view so the SPA's recovery banner (which paints under
        // the native view) becomes visible in the slot.
        try {
            view.setVisible(false);
        }
        catch { /* already gone */ }
        embeddedVisible = false;
        win.webContents.send("embedded-browser:gone", {
            reason: details.reason,
            exitCode: details.exitCode,
        });
    });
}
/** round-256: recover the embedded view after a RENDERER CRASH. A
 *  process-gone renderer usually leaves the webContents object alive but
 *  dead — loadURL on it never recovers, so force a full re-create: drop the
 *  old view (destroy + remove) and build a fresh one via embeddedViewEnsure,
 *  then navigate to the last URL (or a default). */
function embeddedRecover() {
    try {
        if (embeddedView && !embeddedView.webContents.isDestroyed()) {
            embeddedView.webContents.close({ waitForBeforeUnload: false });
        }
        if (embeddedView) {
            try {
                win?.contentView.removeChildView(embeddedView);
            }
            catch { /* already gone */ }
            embeddedView = null;
        }
    }
    catch { /* fall through to ensure() */ }
    const view = embeddedViewEnsure();
    if (!view || view.webContents.isDestroyed())
        return { ok: false };
    // Already decided by loadTarget() when embeddedUrl was set (the ONE load
    // door) — recovery re-loads that decision, it does not re-open the policy.
    // The fallback is about:blank, NOT a third-party home page: recovering a
    // crashed view must not make a request to a search engine to do it. That
    // rule is `embeddedRecoverUrl`'s, in the policy crate.
    const url = (0, summrise_shell_policy_1.embeddedRecoverUrl)(embeddedUrl);
    view.webContents.loadURL(url).catch(() => { });
    // Re-show if the SPA slot is live (bounds were placed before the crash).
    if (embeddedVisible && win && embeddedBounds) {
        view.setBounds(embeddedBounds);
        view.setVisible(true);
        win.contentView.addChildView(view);
    }
    return { ok: true };
}
ipcHandle("embedded-browser:recover", () => embeddedRecover());
ipcHandle("embedded-browser:navigate", (url) => embeddedNavigate(String(url || "")));
ipcHandle("embedded-browser:back", () => embeddedGo(-1));
ipcHandle("embedded-browser:fwd", () => embeddedGo(1));
ipcHandle("embedded-browser:reload", () => embeddedReload());
/** round-251: zoom the REAL embedded view (webContents.setZoomFactor — the
 *  native browser zooms, not a CSS scale). factor: 0.5-3.0. */
ipcHandle("embedded-browser:zoom", (factor) => {
    const view = embeddedView;
    if (!view || view.webContents.isDestroyed())
        return { ok: false };
    // The clamp AND the `Number(factor) || 1` falsy test are the policy crate's, because `zoomFactor(0)`
    // is 100% and not 50% — a boundary that a hand-written `Math.min/Math.max` here got right by
    // accident and a rewrite would not.
    const f = (0, summrise_shell_policy_1.zoomFactor)(factor);
    try {
        view.webContents.setZoomFactor(f);
        return { ok: true, factor: f };
    }
    catch {
        return { ok: false };
    }
});
ipcHandle("embedded-browser:place", (bounds) => {
    embeddedViewPlace(bounds);
    return { ok: true };
});
ipcHandle("embedded-browser:state", () => embeddedState());
// ── THE DSH VIEW: the harness's own UI, embedded in summrise ────────────────────────────────
//
// WHY LOOPBACK AND NOT THE PUBLIC HOSTNAME. The DSH runs on THIS machine (127.0.0.1:18081), and
// this view is a window ON this machine — so loopback is the correct address, not the remote
// one. Going out to the tunnel hostname instead would mean: a Cloudflare round trip for every
// asset, an Access login inside the shell, and a certificate story — for a service that is
// already here. (The spec's warning about loopback applies to the OTHER direction: a page viewed
// REMOTELY resolves 127.0.0.1 to the viewer's machine, which is why the panel's /dsh/ proxy was
// proposed. A native view on the device has no such problem.)
//
// A SECOND view, not a re-use of the browser one: the browser view is a single-tab internet
// browser an AI drives over CDP, and navigating it to the DSH would throw that session away.
let dshView = null;
let dshVisible = false;
let dshBounds = null;
/** The DSH's port. Env first (a deployment may move it), else the component's default — the range test
 *  and the fallback are the policy crate's, the env read is this file's. */
function bootDshPort() {
    return (0, summrise_shell_policy_1.resolveDshPort)(process.env.SUMMRISE_DSH_PORT);
}
/** The ONE load door for this view: its own origin, or nothing.
 *
 *  TWO OWNERS, ONE ANSWER: `isDshUrl` is the URL policy's predicate over its own port list, and this
 *  file evaluates it because that list is state the two wasm modules must not each hold a copy of (see
 *  the import block). The SERIALIZATION of an admitted target — and the refusal of everything else — is
 *  the shell policy's. */
function dshTargetFor(raw) {
    return (0, summrise_shell_policy_1.dshTarget)(raw, (0, summrise_url_policy_1.isDshUrl)(raw));
}
// ── ONE DOOR PER HOST, TAKEN FROM THE AGENT'S OWN TABLE ───────────────────────────────────────────────
// The harness page shows the SELECTED host's own harness, and each host is reached through its own
// forward on this machine's loopback. Those ports are not a shell setting: the agent keeps them, because
// it is what knows which host a forward belongs to. So the shell asks, once at boot, and admits what the
// answer names — never a port of its own choosing, and never a host that is not in the table.
//
// A read that fails leaves the list as it was. A shell that widened its own door list on a failed read is
// exactly how the view ends up pointed somewhere it was not told about.
function loadHarnessDoors() {
    // `agentBase()` rather than a second port read: `setAgentPort` has already run at this point, so the
    // URL policy's base IS what `resolveAgentPort()` would compute — one answer, not two.
    const base = (0, summrise_url_policy_1.agentBase)();
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), HARNESS_FETCH_MS);
    fetch(`${base}/api/workspace/harnesses`, { headers: authHeaders(), signal: controller.signal })
        .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`HTTP ${r.status}`))))
        .then((answer) => {
        const rows = answer?.harnesses;
        // THREE OWNERS, ONE DOOR LIST: the agent NAMES the ports (`local_port` per host), the policy crate
        // says which are usable (`Array.isArray` and `Number(row?.local_port)` are JavaScript operations on
        // a JavaScript value, so they happen inside `harnessDoors`; the RANGE TEST is the same one every
        // other port in the shell goes through), and the URL policy ADMITS them — `addDshPort` dedupes
        // against its own list, which is state this module must not keep a second copy of.
        for (const port of (0, summrise_shell_policy_1.harnessDoors)(rows))
            (0, summrise_url_policy_1.addDshPort)(port);
    })
        .catch(() => {
        /* the list stays as it was: one host, or none */
    })
        .finally(() => clearTimeout(timer));
}
function dshViewEnsure() {
    if (dshView && !dshView.webContents.isDestroyed())
        return dshView;
    const view = new electron_1.WebContentsView({
        webPreferences: {
            contextIsolation: true,
            nodeIntegration: false,
            sandbox: true,
            // No preload: the DSH SPA needs no Node surface, and a view that cannot reach one cannot
            // be talked into using one.
        },
    });
    view.setVisible(false);
    dshVisible = false;
    // The DSH opens links (docs, the operator's own pages) with target=_blank. Deny the POPUP but
    // keep the click meaningful where the door allows it — and the door allows one origin, so in
    // practice an external link is simply dropped rather than silently navigating this view away
    // from the harness.
    view.webContents.setWindowOpenHandler(({ url }) => {
        const decided = (0, summrise_shell_policy_1.dshPopupTarget)(url, (0, summrise_url_policy_1.isDshUrl)(url));
        if (decided !== undefined)
            dshNavigate(decided);
        return { action: "deny" };
    });
    view.webContents.on("render-process-gone", (_e, details) => {
        try {
            view.setVisible(false);
        }
        catch { /* already gone */ }
        dshVisible = false;
        // The SAME signal the browser view sends, for the same reason (round-256 there): without it
        // the pane shows "starting…" forever over a view that will never paint, and the operator has
        // no way to tell a slow load from a dead renderer.
        if (win && !win.isDestroyed()) {
            win.webContents.send("embedded-dsh:gone", { reason: details.reason, exitCode: details.exitCode });
        }
    });
    dshView = view;
    if (win)
        win.contentView.addChildView(view);
    return view;
}
function dshNavigate(url) {
    const target = dshTargetFor(url);
    const view = dshViewEnsure();
    view.webContents.loadURL(target).catch(() => { });
    return { ok: true, url: target };
}
/** Open (or re-focus) the DSH at its configured address. Idempotent.
 *
 *  `dshHome` is the policy crate's and it was spelled TWICE in this file before the port — here and in
 *  `dshRecover` — which is exactly the pair that must not drift: a crash recovery that lands somewhere
 *  other than the harness is a view the operator did not ask for. */
function dshOpen() {
    return dshNavigate((0, summrise_shell_policy_1.dshHome)((0, summrise_url_policy_1.dshBase)()));
}
/** Place the view over the SPA's slot. Empty bounds hide it (the same contract as the browser). */
function dshPlace(bounds) {
    const view = dshViewEnsure();
    if (!win)
        return;
    // The SAME predicate the browser view uses — one `MIN_SLOT_PX`, two call sites, before the port.
    if ((0, summrise_shell_policy_1.slotTooSmall)(bounds)) {
        view.setVisible(false);
        dshVisible = false;
        return;
    }
    dshBounds = bounds;
    view.setBounds(bounds);
    view.setVisible(true);
    dshVisible = true;
    win.contentView.addChildView(view);
}
function dshReload() {
    const view = dshView;
    if (!view || view.webContents.isDestroyed())
        return { ok: false };
    try {
        view.webContents.reload();
        return { ok: true };
    }
    catch {
        return { ok: false };
    }
}
/** Recover after a renderer crash: a dead webContents never recovers, so re-create the view. */
function dshRecover() {
    try {
        if (dshView && !dshView.webContents.isDestroyed())
            dshView.webContents.close({ waitForBeforeUnload: false });
        if (dshView) {
            try {
                win?.contentView.removeChildView(dshView);
            }
            catch { /* already gone */ }
            dshView = null;
        }
    }
    catch { /* fall through to ensure() */ }
    const view = dshViewEnsure();
    if (!view || view.webContents.isDestroyed())
        return { ok: false };
    view.webContents.loadURL((0, summrise_shell_policy_1.dshHome)((0, summrise_url_policy_1.dshBase)())).catch(() => { });
    if (dshVisible && win && dshBounds) {
        view.setBounds(dshBounds);
        view.setVisible(true);
        win.contentView.addChildView(view);
    }
    return { ok: true };
}
function dshState() {
    const view = dshView;
    const wc = view && !view.webContents.isDestroyed() ? view.webContents : null;
    return { ok: true, url: wc ? wc.getURL() : "", title: wc ? wc.getTitle() : "", visible: dshVisible && !!wc };
}
ipcHandle("embedded-dsh:open", () => dshOpen());
ipcHandle("embedded-dsh:place", (bounds) => {
    dshPlace(bounds);
    return { ok: true };
});
ipcHandle("embedded-dsh:state", () => dshState());
// SELECT A HOST: `dshNavigate` already refuses anything `isDshUrl` does not admit, so this channel can
// only reach a door the agent configured — and it answers with the address it actually loaded, which for a
// refusal is `about:blank`. The pane shows that rather than pretending the switch worked.
ipcHandle("embedded-dsh:go", (url) => dshNavigate(String(url)));
ipcHandle("embedded-dsh:reload", () => dshReload());
ipcHandle("embedded-dsh:recover", () => dshRecover());
// Desktop-app settings (auto-launch) — the Settings page toggles this. We
// manage a per-user scheduled task ("SummriseDesktop", onlogon) instead of
// Electron's setLoginItemSettings: in dev mode (electron .) the login-item
// API is unreliable, while schtasks works for the current user without
// elevation. The task runs start-desktop.ps1 (non-elevated → clickable).
const AUTOSTART_TASK = K.AUTOSTART_TASK;
// Resolve the autostart script from the install root (layout v2: the shell
// lives at <install>\components\summrise-desktop-electron\src\, so the script
// is at <install>\scripts\ — process.cwd() depends on how the shell was
// launched and broke the toggle when electron started from another
// directory).
const AUTOSTART_SCRIPT = path.join(INSTALL_ROOT, "scripts", "start-desktop.ps1");
async function autoLaunchTaskExists() {
    // review #4: was sync execSync schtasks — the same hang class that killed
    // electron; route through the bounded async runner.
    const r = await runSchtasks((0, summrise_shell_policy_1.schtasksQueryArgs)(AUTOSTART_TASK));
    return r.ok;
}
/** Run schtasks asynchronously with a hard timeout — the SYNC execSync
 *  variant could hang the main process on a wedged schtasks (observed: the
 *  electron process died when setAutoLaunch ran it inline). Spawn + await
 *  keeps the UI thread free and bounds the call (stage-n). */
function runSchtasks(args) {
    return new Promise((resolve) => {
        try {
            const { spawn } = require("child_process");
            const child = spawn("schtasks", args, { windowsHide: true, stdio: "pipe" });
            // review #5: consume the pipes — a task "already running" message on
            // stdout with no reader EPIPE-kills the child and surfaces a spurious
            // error even though schtasks succeeded.
            child.stdout?.resume();
            child.stderr?.resume();
            const t = setTimeout(() => { try {
                child.kill();
            }
            catch { } resolve({ ok: false, error: "schtasks timed out" }); }, SCHTASKS_TIMEOUT_MS);
            child.on("error", (e) => { clearTimeout(t); resolve({ ok: false, error: String(e) }); });
            child.on("close", (code) => { clearTimeout(t); resolve({ ok: code === 0, error: code === 0 ? undefined : `schtasks exit ${code}` }); });
        }
        catch (e) {
            resolve({ ok: false, error: String(e) });
        }
    });
}
async function autoLaunchTaskSet(enabled) {
    try {
        // THE PLAN IS THE POLICY CRATE'S (`autoLaunchPlan(enabled, exists)`): create when it is asked for and
        // absent, end-then-delete when it is refused and present, and NOTHING when it is already in the
        // state asked for — which is a SUCCESS and not a no-op to be apologised for. This file asks whether
        // the task exists and runs the argv; it decides neither.
        switch ((0, summrise_shell_policy_1.autoLaunchPlan)(!!enabled, await autoLaunchTaskExists())) {
            case "create": {
                // Under SYSTEM, a spawn-array schtasks /create without /ru fails with
                // "no mapping between account names and security IDs" (exit 1) — the
                // interactive user is not resolvable from the service session. A bare
                // STRING invocation happens to work (shell default), but spawn arrays
                // need an explicit account: use Administrator (the d1 console user).
                //
                // THIS PATH CANNOT REPRODUCE WHAT `summrise` REGISTERS, and the difference matters. The CLI
                // gives SummriseDesktop TWO triggers — AtLogOn plus a guarded 5-minute pulse (bin/summrise.js:
                // `New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(3) -RepetitionInterval 5min`, run
                // through desktop-pulse.vbs → ensure-desktop.ps1, which exits when electron is already up).
                // `schtasks /create` has no way to express a repetition interval at all, so what this writes is
                // a LOGON-ONLY task: toggling auto-launch OFF deletes the hardened task, and toggling it ON
                // again replaces it with one that cannot recover a wedged shell. `summrise update` heals it and
                // says so in its log — "desk: SummriseDesktop hardened (guarded 5-min pulse)" — which is how
                // this collision was found in the first place.
                //
                // Doing it properly means New-ScheduledTaskTrigger through PowerShell (the CLI's shape,
                // duplicated in JS) or delegating to the CLI; neither is a one-liner, and the update-time repair
                // bounds the damage to "until the next update". Recorded where the downgrade happens, so the
                // next reader sees the trap instead of rediscovering it from a shell that stopped recovering.
                // `schtasksCreateArgs` carries the quoting trap the crate documents: `schtasks` re-parses the
                // /tr VALUE as a command line, so the script path needs its OWN inner quotes while the spawn
                // array keeps the outer argument intact. It is also where the `/ru Administrator` and the
                // logon-only SHAPE are recorded — the toggle cannot reproduce what `summrise` registers, and the
                // crate says so where the downgrade happens.
                const r = await runSchtasks((0, summrise_shell_policy_1.schtasksCreateArgs)(AUTOSTART_TASK, AUTOSTART_SCRIPT));
                if (!r.ok)
                    return r;
                break;
            }
            case "remove": {
                // End the task first (a RUNNING task's process tree is terminated on delete) then remove it —
                // the current electron instance must survive.
                await runSchtasks((0, summrise_shell_policy_1.schtasksEndArgs)(AUTOSTART_TASK));
                const r = await runSchtasks((0, summrise_shell_policy_1.schtasksDeleteArgs)(AUTOSTART_TASK));
                if (!r.ok)
                    return r;
                break;
            }
            default: break; // "nothing" — already in the state that was asked for
        }
        return { ok: true, enabled };
    }
    catch (e) {
        return { ok: false, error: String(e) };
    }
}
ipcHandle("desktop:get-auto-launch", async () => ({ ok: true, enabled: await autoLaunchTaskExists() }));
// The auto-launch pair is the PERSISTENCE primitive (onlogon schtasks in an
// admin context) — it absolutely cannot be reachable from a foreign frame.
ipcHandle("desktop:set-auto-launch", async (enabled) => autoLaunchTaskSet(!!enabled));
// --- Agent lifecycle (stage-m: Rust owns it; the shell only probes/triggers) ---
// review #2 (HIGH): a raw TCP connect only proves the PORT is held — an
// agent that is wedged (deadlock / OOM-parked) but still listening keeps the
// connect succeeding, so the watchdog's miss counter NEVER reached the gate,
// the wait page never showed, and a stuck device stayed stuck in silence.
// Liveness = GET /api/status answers 200 within the budget.
function agentResponds(timeoutMs = AGENT_PROBE_DEFAULT_MS) {
    return new Promise((res) => {
        let done = false;
        const finish = (v) => { if (!done) {
            done = true;
            res(v);
        } };
        const req = http.get(`${(0, summrise_url_policy_1.agentBase)()}/api/status`, { timeout: timeoutMs }, (r) => {
            r.resume();
            // ANY HTTP response = the accept loop is alive (this is the exact thing
            // the TCP probe could not see). Do NOT require 200: /api/status is
            // token-gated, so a HEALTHY agent answers 401 to the shell's
            // credential-less probe — requiring 200 made a live agent look dead and
            // sent the watchdog into a restart loop (self-caught regression).
            finish((0, summrise_shell_policy_1.statusIsAlive)(r.statusCode));
        });
        req.on("timeout", () => { req.destroy(); finish(false); });
        req.on("error", () => finish(false));
    });
}
/** Start the agent via the SummriseAgent scheduled task — the ONLY sanctioned
 *  spawn path (the task runs the agent as SYSTEM; the agent itself enforces
 *  single-instance via SUMMRISE_NO_PAUSE bind-failure exit). Never spawn the exe
 *  directly from JS. */
async function startAgentTask() {
    // review #1 (HIGH): sync execSync on the wait-page button / IPC / HTTP
    // paths — one wedged schtasks froze the WHOLE main process (the exact
    // hang class already fixed for the watchdog). Same bounded async runner.
    return runSchtasks((0, summrise_shell_policy_1.schtasksRunArgs)(AGENT_TASK));
}
// The SPA asks the shell for agent status / to (re)start the agent task.
// IPC audit #6: shell:agent-status / shell:start-agent had ZERO consumers
// (preload never exposed them; the wait page and SPA use the 9444 HTTP API).
// Dead handlers removed — every exposed channel is attack surface.
// Fallback HTTP path (plain browser / no preload): same core, CORS-open.
const httpServer = http.createServer((req, res) => {
    // THE REQUEST PARSE AND THE ROUTER ARE THE POLICY CRATE'S. `controlPath` resolves the request target
    // against the loopback base the way `new URL(req.url, "http://127.0.0.1")` did, and — the ONE named
    // divergence in that crate — a malformed request target answers `undefined` instead of throwing out of
    // this listener, which in the original took the whole Electron main process with it. `undefined` falls
    // through to the 404 below.
    const pathname = (0, summrise_shell_policy_1.controlPath)(req.url) ?? "";
    const send = (obj, code = 200) => {
        // review #8: was ACAO:* with no origin check — ANY web page open in ANY
        // local browser could POST /api/shell/start-agent or spam
        // /api/browser-session/open (loopback is exempt from mixed-content
        // blocking). Reflect the origin, and reject requests from foreign
        // origins (reads included — the GET routes expose session URLs and
        // device facts). "null" (our data: wait page) and absent
        // (native tooling/curl) stay allowed.
        res.setHeader("access-control-allow-origin", (0, summrise_shell_policy_1.corsAllowOrigin)(req.headers.origin));
        res.setHeader("access-control-allow-methods", K.CORS_ALLOW_METHODS);
        res.setHeader("access-control-allow-headers", K.CORS_ALLOW_HEADERS);
        res.setHeader("vary", K.CORS_VARY);
        res.writeHead(code, { "content-type": "application/json", "cache-control": "no-store" });
        res.end(JSON.stringify(obj));
    };
    // A PREFLIGHT IS ANSWERED BEFORE THE VETO, and that ordering is the policy crate's
    // (`controlIsPreflight`) rather than a detail of this function: moving the check below the veto would
    // start 403-ing legitimate CORS preflights and the plain-browser path would stop working with no other
    // symptom.
    if ((0, summrise_shell_policy_1.controlIsPreflight)(req.method ?? ""))
        return send({ ok: true });
    // Foreign-origin veto applies to READS too: the GET routes
    // (/api/browser-session/list, /api/shell/agent-status,
    // /api/shell/icon-status) expose session URLs and device facts to any
    // local-browser page (loopback is exempt from mixed-content blocking).
    // Intentionally-allowed origin set: the desktop SPA
    // (127.0.0.1/localhost), file://, "null" (our data: wait page), and
    // absent (native tooling/curl). NOTE: chrome-extension:// is NOT in
    // this set — no live extension caller exists (the extension only talks
    // to its configured gateway origin, never to 127.0.0.1:9444), so the
    // veto stays as-is; an extension caller needs an explicit entry here.
    // THE DECISION ITSELF IS RUST (agent/summrise-url-policy) and was an inline regex with NO `$` ANCHOR,
    // so `http://127.0.0.1.evil.com` passed — the same class as the `startsWith(BASE)` bug that crate
    // exists to kill.
    if (!(0, summrise_url_policy_1.controlOriginOk)(req.headers.origin)) {
        return send({ ok: false, error: K.FORBIDDEN_ORIGIN }, 403);
    }
    try {
        // THE ROUTE TABLE IS THE POLICY CRATE'S, INCLUDING ITS TWO ASYMMETRIES, which are the original
        // behaviour and not accidents to tidy: `/list`, `/agent-status` and `/icon-status` have NO method
        // guard (a POST reaches the same reader), while `/open`, `/close` and `/start-agent` require POST and
        // a GET to one of them falls through to the 404.
        switch ((0, summrise_shell_policy_1.controlRoute)(req.method ?? "", pathname)) {
            case "browser-open": return send(browserOpen((0, summrise_shell_policy_1.controlQuery)(req.url, "url") || K.ABOUT_BLANK));
            case "browser-close": return send(browserClose((0, summrise_shell_policy_1.controlQuery)(req.url, "id") || ""));
            case "browser-list": return send(browserList());
            case "start-agent": return (async () => send(await startAgentTask()))();
            case "agent-status": return (async () => send({ ok: true, running: await agentResponds(CONTROL_PROBE_MS) }))();
            case "icon-status": return send({ ok: true, icons: iconReport,
                native: {
                    tray: probeImage(String(iconReport["tray"]?.path || "")),
                    window: probeImage(String(iconReport["window"]?.path || "")),
                } });
            default: send({ ok: false, error: K.NOT_FOUND }, 404);
        }
    }
    catch (e) {
        send({ ok: false, error: String(e) }, 500);
    }
});
if (gotTheLock) {
    // TASKBAR GROUPING AND THE TASKBAR GROUP'S ICON — HALF ONE OF TWO, AND THIS IS THE HALF
    // THAT IS A PRECONDITION RATHER THAN A HINT.
    // Microsoft's rule for System.AppUserModel.RelaunchIconResource is a three-step lookup:
    // the property ON THE WINDOW; else "the system attempts to find a shortcut with the same
    // AppUserModelID, and pins that shortcut to the taskbar to represent the window"; else
    // "the backing executable of the process that owns it is used". Which step is reachable
    // at all depends on THIS call — the same page: the shortcut lookup happens only "If an
    // explicit AppUserModelID is set on the window", and without one the property "is ignored
    // and the window is grouped and pinned as if it were part of its owning process".
    // This shell is launched UNPACKAGED (`electron.exe .` out of the staged runtime), so
    // step three is stock electron.exe — the Electron logo. Removing this call (d4edbb64)
    // could not fix that: it deleted the caller's half of an association whose OTHER half is
    // the shortcut, which left step three as the only reachable step in BOTH states. The
    // shortcut half is written in agent/summrise-agent-npm/src/summrise.ts (see DESKTOP_AUMID
    // there) as System.AppUserModel.ID — the same string — plus RelaunchIconResource pointing
    // at the sunrise .ico. KEEP THE TWO IN STEP; the literal below is a copy because this file
    // is emitted to plain JS in two packages and cannot import the CLI's module.
    // THE VALUE IS NOT THE ONE THIS FILE USED BEFORE ("…summrise.agent"), and that is the
    // measured half of the fix rather than a rename. On desktop-14rjcr8, with that string set
    // on the window AND read back off the shortcut, the taskbar still drew the Electron logo:
    // releases up to 1.2.489 had already resolved that AUMID — set with no shortcut anywhere —
    // against the backing executable and kept the answer, and `shell:AppsFolder` still lists it
    // as an app named "Electron". The same machine drew the SUNRISE under a string it had never
    // seen, with every other variable held. DESKTOP_AUMID carries the two-row table.
    // (The same shortcut association is also the documented precondition for the hide-to-tray
    // toast below: Electron requires a Start Menu shortcut carrying the ID and a
    // ToastActivatorCLSID, and this one carries neither — unchanged by this call.)
    // THE VALUE, THE PLATFORM TEST AND THE TWO REPORT STRINGS ARE THE POLICY CRATE'S. It is a COPY of the
    // CLI's `DESKTOP_AUMID` and the two must stay in step — `agent/tests/shared_literals.rs` now compares
    // them from both sources and refuses a disagreement, which is what "KEEP THE TWO IN STEP" needed to
    // stop being a comment. `app.setAppUserModelId` is this file's.
    try {
        if ((0, summrise_shell_policy_1.usesAppUserModelId)(process.platform))
            electron_1.app.setAppUserModelId(K.DESKTOP_AUMID);
        iconReport["appUserModelId"] = (0, summrise_shell_policy_1.aumidReport)(process.platform);
    }
    catch {
        iconReport["appUserModelId"] = K.AUMID_SET_FAILED;
    }
    electron_1.app.whenReady().then(async () => {
        // Custom-port installs: pin every origin predicate + probe/load URL to
        // the agent's actual bind port BEFORE any window or probe exists.
        (0, summrise_url_policy_1.setAgentPort)(bootAgentPort());
        // The DSH view's door, pinned the same way and for the same reason: a predicate that ran
        // before this line would check a port nothing is listening on.
        (0, summrise_url_policy_1.setDshPort)(bootDshPort());
        // Every host's forward is a door, admitted from the agent's table rather than from configuration here.
        loadHarnessDoors();
        // review #7: with no handler Electron AUTO-GRANTS every permission
        // request (media/geolocation/clipboard) — deny by default for all
        // windows, esp. the remote-browser ones loading arbitrary pages.
        try {
            electron_1.session.defaultSession.setPermissionRequestHandler((_wc, _perm, cb) => cb(false));
            electron_1.session.defaultSession.setPermissionCheckHandler(() => false);
        }
        catch { /* non-fatal */ }
        httpServer.listen(K.CTRL_PORT, "127.0.0.1");
        console.log(`[summrise] browser-session control: http://127.0.0.1:${K.CTRL_PORT}`);
        // Native application menu (stage-l): menu commands → SPA via summrise-menu.
        electron_1.Menu.setApplicationMenu(buildMenu());
        win = new electron_1.BrowserWindow({
            width: 1200, height: 800, title: "Summrise",
            // NO WHITE FLASH (window-creation honesty, round-275). The window used to be
            // created VISIBLE, before loadDesktop() runs its ~800 ms /api/status probe and
            // before the first loadURL — so every launch painted a blank white frame and then
            // whatever the agent turned out to be. show:false keeps it off screen until the
            // first document has painted (ready-to-show, below); backgroundColor paints that
            // first frame in the shell's own ink instead of Chromium's white, which is the one
            // of the two documents that had NOT already fixed this (the wait page
            // hardcoded background:#111, and the SPA's chrome is dark).
            show: false,
            backgroundColor: "#111",
            // Taskbar + title-bar icon: without this Windows shows the stock
            // electron.exe icon (the running binary is stock Electron). PNG —
            // see windowIcon() on why not .ico here.
            ...(windowIcon() ? { icon: windowIcon() } : {}),
            webPreferences: {
                preload: path.join(__dirname, "preload.js"),
                contextIsolation: true,
                nodeIntegration: false,
                // stage-n preload audit LOW: defense-in-depth — the preload is
                // static/safe, but sandbox:true removes any Node escape path.
                sandbox: true,
                // round-274 (device-caught): the window was hidden (hide-to-tray /
                // background session) and the SPA's visibilityState flipped to
                // "hidden" — Chromium then stops requestAnimationFrame, and xterm's
                // DOM renderer (rAF-driven) silently stopped painting: every
                // terminal went blank while the AI kept operating. backgroundThrottling:
                // false keeps rAF/timers running for hidden windows, so the panel
                // always renders regardless of window visibility.
                backgroundThrottling: false,
            },
        });
        // The other half of show:false — the first paint shows the window. It fires on the
        // first document to paint (the SPA when the agent answers, the wait page when it does
        // not), and the handler is registered in the same tick as the constructor, before any
        // loadURL can happen, so there is no window in which the event is missed.
        win.once("ready-to-show", () => { win?.show(); });
        // review #6 (MED) TOP RISK: the main window carries the summriseDesktop/
        // summriseBrowser preload bridge (setAutoLaunch → schtasks /create …
        // -File <ps1>, browser-session:open). Without a navigation veto a
        // compromised/redirected agent page — or any window.open target — could
        // load an ARBITRARY origin into this privileged window and drive those
        // IPC channels for persistence. Pin the main window to its own origin
        // (the SPA is served from BASE; the wait page is a data: URL).
        win.webContents.on("will-navigate", (e, url) => {
            // audit #1: PARSED-origin veto. The data: carve-out was unnecessary
            // (programmatic loadURL — including the wait page — never fires
            // will-navigate) and only widened the hole.
            if (!(0, summrise_url_policy_1.isBaseOrigin)(url))
                e.preventDefault();
        });
        // round-258 (device-caught): CDP-driven navigation (an attached
        // playwright/AI) BYPASSES will-navigate — a browser_navigate call
        // hijacked the main window to qq.com and the panel vanished. Tripwire:
        // the moment the MAIN window lands anywhere that is not the base origin
        // or the wait page, snap it straight back to the desktop SPA. The SPA's
        // own in-page router never fires did-navigate to a different origin, so
        // this cannot fight legitimate panel use.
        let snappingBack = false;
        win.webContents.on("did-navigate", (_e, url) => {
            if (snappingBack)
                return;
            // Parsed-origin + parsed-pathname allow-list: the string startsWith was the exact class IPC audit
            // #1 flagged, so `isDesktopSpaUrl` (the URL policy's) supplies that half and the policy crate
            // composes it with the two literal carve-outs — the data: wait page and about:blank.
            //
            // **THE PORT DELETED A DEAD LINE HERE.** The original read
            //     if (isDesktopSpaUrl(url) || url.startsWith("data:") || url === "about:blank") return;
            //     if (url === "about:blank") return;
            // and the second `about:blank` test is UNREACHABLE: the first already returns for it. It read as a
            // second guard and had never run.
            if ((0, summrise_shell_policy_1.tripwireAllows)(url, (0, summrise_url_policy_1.isDesktopSpaUrl)(url)))
                return;
            console.log((0, summrise_shell_policy_1.tripwireLog)(url));
            snappingBack = true;
            win?.loadURL(`${(0, summrise_url_policy_1.agentBase)()}/desktop/`).catch(() => { }).finally(() => {
                setTimeout(() => { snappingBack = false; }, TRIPWIRE_BACKOFF_MS);
            });
        });
        // review #7: a target=_blank from the SPA must NOT get a preload-bearing
        // window; route it to a sandboxed browser session instead.
        win.webContents.setWindowOpenHandler(({ url }) => {
            browserOpen(url);
            return { action: "deny" };
        });
        // stage-n: load /desktop/ only when the agent is actually listening.
        // A blind loadURL fails white-screen when the SummriseAgent task is down
        // (startup race, crash, update mid-swap); poll 18080 and retry with
        // exponential backoff (2s → 4s → 8s → 30s cap) so a dead agent doesn't
        // spam retries.
        // stage-n: load /desktop/ only when the agent is actually LISTENING AND
        // ANSWERING. The probe must be HTTP liveness (agentResponds), not the raw
        // TCP connect (portBusy): review #2 established that a wedged-but-
        // listening agent holds the port yet never answers — with portBusy here
        // agentReady() stayed true, so the miss counter below NEVER incremented
        // and the watchdog could not fire (the device stayed stuck in silence,
        // the exact failure the watchdog exists to prevent).
        const agentReady = async () => agentResponds(AGENT_PROBE_BOOT_MS);
        // stage-n: the wait page carries a "Start Agent" action — the header
        // comment promised it but it never existed. The button calls the shell's
        // own /api/shell/start-agent (schtasks /run SummriseAgent — the only
        // sanctioned spawn path). This turns a dead-end white screen into a
        // recoverable state when the SummriseAgent task is down (crash, stopped
        // task, update mid-swap).
        //
        // REWRITTEN (round 276). Three defects, all of them measured on the
        // device's own window at 1200x761, and all three are the same defect
        // wearing different clothes: THE PAGE ASSERTED THINGS IT HAD NOT CHECKED.
        //
        //   1. The heading said "Summrise Agent is not running". Nothing on this
        //      path checked that. `loadDesktop` reaches this page when a request
        //      to one address does not ANSWER — the agent may be running and
        //      wedged, on another port, mid-restart, or still booting. The file's
        //      own comment above `agentReady` says a wedged-but-listening agent
        //      "holds the port yet never answers", which is the case the heading
        //      got wrong. It now names the observation (a request went out and
        //      nothing came back) and the address it went to.
        //   2. The subtext said "waiting for …" and then never showed the wait.
        //      No attempt count, no clock — so a four-second boot and a six-hour
        //      corpse rendered identically. It now carries both, and both TICK
        //      (below), so the operator can tell patience from action.
        //   3. The button threw its answer away. `/api/shell/start-agent` returns
        //      `{ok, error}` — `startAgentTask()` is `runSchtasks([...])` — and
        //      the page did `await fetch(...)` and printed "started" regardless,
        //      so a REFUSED start read exactly like an accepted one. It reads the
        //      body now and says which happened.
        //
        // AND THE PAGE OWNS ITS OWN CLOCK, which is why `loadWaitPage` below will
        // not re-load a page that is already showing: a reload every 2-30 s wiped
        // the button's answer before it could be read, and re-rendered the
        // evidence from zero. The page polls the shell's own /api/shell/agent-status
        // (the shell is alive by construction — it is what draws this page) and
        // navigates itself to the SPA the moment the agent answers.
        //
        // THE COLOURS ARE THE DARK THEME'S TOKEN VALUES, LITERALLY, AND THAT IS
        // DELIBERATE: this page is drawn while the agent is mute, so it cannot
        // load the SPA's token sheet — but a fallback using its own greys is a
        // different product's screen. Values copied from
        // panel-react/src/styles/tokens.css `body[data-theme="dark"]`:
        // --bg #131418, --chrome-ink #ecedef, --chrome-ink-dim #a2a3ac,
        // --accent-solid #b03a0a, --accent-solid-hover #a63308, --accent-fg #fff,
        // --chrome-danger-ink #ff8787.
        //
        // NO BACKTICKS AND NO `${` BELOW except the two interpolations that are
        // meant: this is a template literal, and the incident class the pre-commit
        // hook was built for is a stray backtick inside one.
        const waitPageHtml = (checks) => `<!doctype html><meta charset="utf-8"><title>Summrise</title>
      <style>
        body{font-family:system-ui,sans-serif;background:#131418;color:#ecedef;display:flex;align-items:center;justify-content:center;height:100vh;margin:0}
        div{text-align:center;max-width:640px;padding:0 24px}
        h2{margin:0;font-size:24px;font-weight:600}
        p{margin:10px 0 0;font-size:15px;color:#a2a3ac}
        #action{margin-top:12px;font-size:14px}
        #action.failed{color:#ff8787}
        button{margin-top:18px;padding:10px 22px;font-size:15px;border-radius:8px;border:0;background:#b03a0a;color:#fff;cursor:pointer}
        button:hover:not(:disabled){background:#a63308}
        button:disabled{opacity:.5;cursor:default}
      </style>
      <div>
        <h2>The Summrise Agent isn&#39;t answering</h2>
        <p id="status">no reply from ${(0, summrise_shell_policy_1.agentHostLabel)((0, summrise_url_policy_1.agentBase)())}</p>
        <p id="action" hidden></p>
        <button id="start">Start Agent</button>
      </div>
      <script>
        var BASE = ${JSON.stringify((0, summrise_shell_policy_1.agentHostLabel)((0, summrise_url_policy_1.agentBase)()))};
        var CTRL = "http://127.0.0.1:9444";
        var btn = document.getElementById("start");
        var st = document.getElementById("status");
        var act = document.getElementById("action");
        var checks = ${checks};
        var lastCheckAt = Date.now();
        function say(el, text, failed) {
          el.hidden = false;
          el.textContent = text;
          el.className = failed ? "failed" : "";
        }
        function age(ms) { return ms < 1500 ? "just now" : Math.round(ms / 1000) + "s ago"; }
        function paint() {
          st.textContent = "no reply from " + BASE + " \\u00b7 last check " + age(Date.now() - lastCheckAt)
            + " \\u00b7 " + checks + (checks === 1 ? " check" : " checks");
        }
        paint();
        setInterval(paint, 1000);
        function poll() {
          fetch(CTRL + "/api/shell/agent-status").then(function (r) { return r.json(); }).then(function (j) {
            checks += 1; lastCheckAt = Date.now(); paint();
            if (j && j.running) { location.replace("http://" + BASE + "/desktop/"); return; }
            setTimeout(poll, 2000);
          }).catch(function () {
            checks += 1; lastCheckAt = Date.now(); paint();
            setTimeout(poll, 2000);
          });
        }
        setTimeout(poll, 2000);
        var busy = false;
        btn.addEventListener("click", async function () {
          if (busy) return;
          busy = true; btn.disabled = true;
          say(act, "asking the SummriseAgent task to start\\u2026", false);
          try {
            var r = await fetch(CTRL + "/api/shell/start-agent", { method: "POST" });
            var j = null;
            try { j = await r.json(); } catch (e) { j = null; }
            if (j && j.ok) {
              say(act, "the SummriseAgent task was started \\u2014 waiting for it to answer", false);
            } else {
              say(act, "the start request FAILED: " + ((j && j.error) || ("HTTP " + r.status)), true);
            }
          } catch (e) {
            say(act, "the start request did not reach the shell: " + e, true);
          }
          setTimeout(function () { busy = false; btn.disabled = false; }, 3000);
        });
      </script>`;
        const loadWaitPage = (checks) => {
            // DO NOT RE-LOAD A PAGE THAT IS ALREADY SHOWING (see the note above): the
            // reload wiped the button's answer and reset the evidence line, which is
            // what made the only control on this screen unfalsifiable.
            // `isWaitPage` is the policy crate's, and the prefix test is SAFE because the URL it tests is one
            // this file built — no page can make itself match.
            if ((0, summrise_shell_policy_1.isWaitPage)(win?.webContents.getURL() ?? ""))
                return;
            win?.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(waitPageHtml(checks))}`).catch(() => { });
        };
        // THE BACKOFF IS THE POLICY CRATE'S: double, capped at `RETRY_CAP_MS`, and reset to `RETRY_START_MS`
        // on a successful probe.
        let retryMs = RETRY_START_MS;
        const nextRetry = () => { retryMs = (0, summrise_shell_policy_1.nextRetryMs)(retryMs); return retryMs; };
        const resetRetry = () => { retryMs = RETRY_START_MS; };
        // NO setVersionTitle() HERE, DELETED. It fetched /api/status with the device token and
        // called win.setTitle("Summrise — v1.2.x"), and that title NEVER REACHED THE SCREEN: the
        // SPA owns document.title (`useAttentionTitle` assigns it on every attention change,
        // panel-react/src/hooks/useAttention.ts:96 — plain "Summrise Agent" in the desktop window)
        // and no page-title-updated handler on THIS window stops it, so the renderer's title
        // replaced the native one on the first render. The version it wanted to show is already on
        // screen, in the panel's own status bar (DesktopShell's .desktop-status) and in the tray
        // tooltip, both of which read the same /api/status. A native title that only the process
        // could read was also the LAST credentialed fetch on this path — the tray keeps its own.
        // stage-n: SELF-HEAL watchdog. The wait-page button needed a human, but
        // when the AGENT is down the cloudflared tunnel (agent-supervised) dies
        // too — a remote operator then has NO channel left to click anything
        // (happened on d1: a killed script had run `schtasks /End` before its
        // `/Run`, and the device stayed dark). After ≥5 consecutive failed
        // probes (~60 s of silence — comfortably past an update swap's ~10 s
        // stop/restart window, so no race with the sanctioned updater) the
        // shell runs the only sanctioned spawn path itself, at most once per
        // 5 minutes. The async schtasks (runSchtasks) keeps the main process
        // free — the same hang class that killed the old auto-launch toggle.
        let agentMissCount = 0;
        let lastAutoStartAt = 0;
        const loadDesktop = async () => {
            if (await agentReady()) {
                agentMissCount = 0;
                resetRetry();
                win?.loadURL(`${(0, summrise_url_policy_1.agentBase)()}/desktop/`).catch(() => { });
            }
            else {
                // THE MISS IS COUNTED BEFORE THE PAGE IS HANDED THE NUMBER, because this
                // failed probe IS one: the count the operator reads is the number of
                // times the shell has asked and got nothing, including this one.
                agentMissCount += 1;
                loadWaitPage(agentMissCount);
                // THE GATE IS THE POLICY CRATE'S: five consecutive misses AND at least five minutes since the
                // last self-start. The argv is its too, which is what makes the task name this runs and the name
                // the CLI registers one fact instead of two.
                if ((0, summrise_shell_policy_1.watchdogShouldStart)(agentMissCount, lastAutoStartAt, Date.now())) {
                    lastAutoStartAt = Date.now();
                    agentMissCount = 0;
                    void runSchtasks((0, summrise_shell_policy_1.schtasksRunArgs)(AGENT_TASK)).then((r) => {
                        console.log((0, summrise_shell_policy_1.watchdogLog)(r.ok, r.error));
                    });
                }
                setTimeout(() => { loadDesktop(); }, nextRetry());
            }
        };
        // Retry loop: did-fail-load (agent went down mid-load) or a still-down
        // agent re-runs loadDesktop; each failure schedules exactly one retry.
        // review #3: a same-URL reload dispatches did-fail-load(-3, ERR_ABORTED)
        // on the MAIN frame, and subframe failures fired too — each stacked
        // ANOTHER loadDesktop chain (accelerated miss counting + duplicate
        // loadURLs). Only a real main-frame failure (not ABORTED) should retry.
        win.webContents.on("did-fail-load", (_e, errorCode, _desc, _url, isMainFrame) => {
            // `-3` is ERR_ABORTED and it is EXCLUDED on purpose: a same-URL reload dispatches it on the MAIN
            // frame, and retrying it stacked a second `loadDesktop` chain with accelerated miss counting. The
            // predicate is the policy crate's.
            if (!(0, summrise_shell_policy_1.shouldRetryLoad)(!!isMainFrame, errorCode))
                return;
            setTimeout(() => { loadDesktop(); }, nextRetry());
        });
        // stage-n: once the SPA finished loading, give its React effect time to
        // register the summrise-menu listener, then flush any queued menu commands.
        win.webContents.on("did-finish-load", () => {
            if (menuFlushTimer)
                clearTimeout(menuFlushTimer);
            menuFlushTimer = setTimeout(flushMenuQueue, MENU_FLUSH_MS);
        });
        await loadDesktop();
        // round-259: eagerly create the EMBEDDED view at startup (hidden) so an
        // attached playwright/AI ALWAYS has its own dedicated page to drive —
        // without it, playwright grabbed the MAIN window (now tripwired) or the
        // user had to open the Browser page first. The view is hidden until the
        // SPA's Browser page reports bounds (embeddedViewPlace shows it).
        // NO EAGER NAVIGATION: this used to loadURL("https://www.bing.com"), i.e. a
        // request to a third party on EVERY launch before anyone asked for a page.
        // The view is still created — about:blank is a first-class CDP target (the
        // reason it exists here is that a target be available, not that a page be
        // loaded), and the user/AI navigates it next (loadTarget is the one door).
        try {
            embeddedViewEnsure();
        }
        catch { /* non-fatal */ }
        console.log(`[summrise] CDP endpoint: ${(0, summrise_shell_policy_1.cdpEndpoint)(K.CDP_PORT)} (playwright connectOverCDP)`);
        // stage-n: CDP self-check — if 9333 is occupied by another process
        // (a second browser/electron), remote-debugging-port silently fails and
        // AI driving would hit the WRONG target. Probe /json/version and verify
        // the User-Agent belongs to this app.
        // THE QUESTION IS THE POLICY CRATE'S, THE REQUEST IS THIS FILE'S. It exists because "if 9333 is
        // occupied by another process (a second browser/electron), `remote-debugging-port` silently fails and
        // AI driving would hit the WRONG target" — so `/json/version` is probed and the `User-Agent` is
        // compared against this app's marker. The three messages are the crate's so that the wording a person
        // reads off a device's log is pinned by a test.
        (async () => {
            try {
                const ctrl = new AbortController();
                const t = setTimeout(() => ctrl.abort(), CDP_CHECK_MS);
                const r = await fetch(`http://127.0.0.1:${K.CDP_PORT}/json/version`, { signal: ctrl.signal });
                clearTimeout(t);
                if (r.ok) {
                    const ua = (0, summrise_shell_policy_1.cdpUserAgent)(await r.text());
                    if ((0, summrise_shell_policy_1.cdpUserAgentOurs)(ua)) {
                        console.log((0, summrise_shell_policy_1.cdpSelfCheckOk)(K.CDP_PORT));
                    }
                    else {
                        console.warn((0, summrise_shell_policy_1.cdpWarningForeign)(K.CDP_PORT, ua));
                    }
                }
                else {
                    console.warn((0, summrise_shell_policy_1.cdpWarningNotResponding)(K.CDP_PORT));
                }
            }
            catch {
                console.warn((0, summrise_shell_policy_1.cdpWarningUnreachable)(K.CDP_PORT));
            }
        })();
        win.on("close", (e) => {
            if (!electron_1.app.isQuitting) {
                e.preventDefault();
                win?.hide();
                // stage-n: first close hides to tray — tell the user once so they
                // don't think the app exited (a silent hide reads as 'closed').
                if (!hideNotified) {
                    hideNotified = true;
                    try {
                        new electron_1.Notification({ title: "Summrise", body: "Summrise is still running in the system tray." }).show();
                    }
                    catch { /* notifications unavailable — harmless */ }
                }
            }
        });
        // Windows requires .ico for tray; macOS/Linux accept .png.
        const iconPath = appIcon();
        tray = new electron_1.Tray(iconPath ? iconPath : electron_1.nativeImage.createEmpty());
        console.log(`[summrise] icons ${JSON.stringify(iconReport)}`);
        // review #f: single/double-click on the tray icon did nothing (only the
        // context-menu "Open" worked).
        tray.on("click", () => focusMain());
        // stage-n: tray reflects live agent state — tooltip + a status line in
        // the menu, refreshed on a 30s poll and on demand (menu open re-checks).
        let trayAgentRunning = false;
        // stage-n: guards the single loadDesktop re-entry loop (see tray hook).
        let agentWatchActive = false;
        /** WHAT THE TRAY KNOWS, carried between polls: the facts the last `/api/status` answer left behind.
         *  The RULE — a poll that fails, or an answer that omits a field, KEEPS what the last one said — is
         *  the policy crate's; the STATE is this file's, because a poll is the only thing that observes it.
         *  The two formatters this file used to carry (`fmtUptime`, `fmtClock`) are that crate's too, so
         *  there is no clock face or uptime string built here at all. */
        let trayFacts = { version: "", uptime: "", sessions: 0, cpu: null, mem: null };
        /** WHEN /api/status last ANSWERED (null = it never has since launch) — the tray's
         *  tooltip reports this instead of asserting a state it cannot date. The policy crate decides what
         *  it MEANS; this file only records it. */
        let trayLastAnsweredAt = null;
        const refreshTray = async () => {
            // HTTP liveness, not the TCP probe (see agentReady above): a wedged-
            // but-listening agent must show as STOPPED, not "running".
            const running = await agentResponds(AGENT_PROBE_TRAY_MS);
            const observedAt = Date.now();
            // THE PARSE, THE KEEP-LAST RULE, THE VITALS LINE AND THE TOOLTIP'S WORDING ARE ONE CALL INTO THE
            // POLICY CRATE, and this file hands over the RAW BODY rather than a parsed object: `r.json()`
            // throwing into a `catch { keep last }` IS the keep-last rule, so the parse belongs where the rule
            // is. `r.text()` moves the throw across the boundary.
            let statusText = "";
            if (running) {
                try {
                    const ctrl = new AbortController();
                    const t = setTimeout(() => ctrl.abort(), STATUS_FETCH_MS);
                    const r = await fetch(`${(0, summrise_url_policy_1.agentBase)()}/api/status`, { signal: ctrl.signal, headers: authHeaders() });
                    clearTimeout(t);
                    if (r.ok)
                        statusText = await r.text();
                }
                catch { /* keep last */ }
            }
            // THE LOCAL TIME ZONE IS A HOST FACT AND IT IS PASSED PER INSTANT: wasm has no clock and no zone
            // database, and the two clock faces the tooltip can print are up to a DST boundary apart — one
            // offset for both would put the wrong hour on one of them for exactly one poll, which is the
            // "a timestamp does not decay" promise the wording was rewritten to keep.
            const next = JSON.parse((0, summrise_shell_policy_1.refreshTrayHealth)(JSON.stringify({
                running,
                statusText,
                prev: trayFacts,
                observedAt,
                lastAnsweredAt: trayLastAnsweredAt,
                tzOffsetMin: new Date(observedAt).getTimezoneOffset(),
                lastTzOffsetMin: new Date(trayLastAnsweredAt ?? observedAt).getTimezoneOffset(),
            })));
            trayFacts = next.facts;
            trayLastAnsweredAt = next.lastAnsweredAt;
            const health = next.health;
            trayAgentRunning = running;
            // stage-n: the agent died WHILE the window was showing the SPA (the
            // wait page only renders when the boot probe fails). Swap the window
            // to the wait page — its "Start Agent" button is now actually visible
            // (user report: the button could never be reached) — and re-enter the
            // loadDesktop retry loop, whose watchdog auto-runs `schtasks /run
            // SummriseAgent` after ~60 s of misses. One loop guard so repeated tray
            // polls cannot stack retries.
            //
            // The CONJUNCTION is the policy crate's; the ORIGIN half is the URL policy's, evaluated only when
            // the window is live so the original's short-circuit survives (a destroyed window never reaches a
            // `getURL()`).
            const liveWindow = win && !win.isDestroyed() ? win : null;
            if ((0, summrise_shell_policy_1.trayShouldWatch)(running, agentWatchActive, liveWindow !== null, liveWindow ? (0, summrise_url_policy_1.isBaseOrigin)(liveWindow.webContents.getURL()) : false)) {
                agentWatchActive = true;
                void loadDesktop();
            }
            if (running && agentWatchActive)
                agentWatchActive = false;
            // THE TOOLTIP NAMES THE OBSERVATION, NOT A VERDICT (round-275), and the crate computes it: it used
            // to print "Agent stopped" as present tense from ONE un-timestamped probe, recomputed every 30 s —
            // half a minute stale while reading as now. The WHEN is a CLOCK rather than "answered 12s ago"
            // because a tooltip is a string written once per poll and Electron gives it no way to recompute; a
            // relative age would freeze and read as "now" for the next 30 s. A timestamp does not decay.
            tray.setToolTip(`Summrise — ${health}`);
            // THE FOUR "New …" ROWS ARE GONE (round-275): they duplicated the desktop
            // header's `+ New` menu item for item (pty/ssh/serial/browser), one click
            // cheaper than the tray path, which had to restore and focus the window
            // first anyway. "Open" is the tray's job; creating sessions is the app's.
            tray.setContextMenu(electron_1.Menu.buildFromTemplate([
                { label: "Open", click: () => { focusMain(); } },
                { type: "separator" },
                { label: health, enabled: false },
                { type: "separator" },
                { label: "Quit", click: () => { electron_1.app.isQuitting = true; electron_1.app.quit(); } },
            ]));
        };
        refreshTray();
        setInterval(() => { refreshTray(); }, TRAY_POLL_MS);
    });
}
electron_1.app.on("before-quit", () => {
    electron_1.app.isQuitting = true;
    // stage-n: close every browser-session window explicitly — app.quit()
    // tears down the main window but independent BrowserWindows with pending
    // beforeunload handlers can stall or leak on exit.
    for (const [id, bw] of browserSessions) {
        if (!bw.isDestroyed())
            bw.destroy();
        browserSessions.delete(id);
        browserTargets.delete(id);
    }
});
