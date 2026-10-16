"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.browserSessionsBind = browserSessionsBind;
exports.browserOpen = browserOpen;
exports.browserClose = browserClose;
exports.browserList = browserList;
exports.browserCloseAll = browserCloseAll;
// ── Browser-session windows: the cluster that owns them (round-247) ────────────────────────────
// `main.ts`'s browser-session section, moved out whole: the three operations an AI drives over IPC or
// the loopback control server (`browserOpen`, `browserClose`, `browserList`), the session table they
// read and write, and the shutdown sweep that closes it. What those operations DECIDE is not here, and
// never was: the reuse/evict plan is `agent/summrise-shell-policy`'s (`planBrowserOpen`), the window id
// is its `browserId`, the CDP endpoint they advertise is its `cdpEndpoint`, and the cap is its
// `MAX_BROWSER_WINDOWS`. This file applies those answers to real `BrowserWindow`s.
//
// ── WHY THIS IS A FILE OF ITS OWN, AND THE THREE THINGS IT DOES NOT OWN ────────────────────────
// The survey that found it measured that nothing here touches `tray`, `Tray`, `Menu`, `mainWindow`,
// `ipcMain` or the `win` variable — and it also measured what a survey of NAMES misses: the section has
// THREE dependencies on `main.ts`, and they are why this file takes a seam rather than an import.
//   * the ONE load door (`loadTarget`) — `main.ts`'s single call site of the URL policy's
//     `sanitizeBrowserUrl`. Every URL a session window opens must go through it, never around it.
//   * the window icon (`windowIcon`) — the shell's own resolution of `windowIconName()` against the
//     install tree, which is `main.ts`'s business (it records what it resolved for
//     `/api/shell/icon-status`) and is shared with the main window.
//   * the window cap (`maxBrowserWindows`) — `K.MAX_BROWSER_WINDOWS`, the boot table `main.ts` parses
//     once from the policy crate. One number, fixed at boot: a VALUE, not a getter.
// Importing any of the three from `main.ts` would make `main.ts` require this module and this module
// require `main.ts`: a cycle whose failure mode is a half-initialised module rather than an error. So
// `main.ts` binds all three ONCE (`browserSessionsBind`) and this module reads them through `deps()`.
//
// THE IPC REGISTRATIONS DELIBERATELY DID NOT MOVE WITH IT. They go through `main.ts`'s ONE door
// (`ipcHandle`, which applies the frame check and is the file's only `ipcMain.handle` call), and the
// channel set is pinned against the preload's by `test/ipc-door.test.mjs`. A handler registered here
// would be a second door, so `main.ts` keeps the three `ipcHandle("browser-session:…")` lines and calls
// in — and with them the THREE other call sites the section always had: the loopback control server's
// `browser-open`/`browser-close`/`browser-list` routes (the SPA in a plain browser) and the main
// window's `setWindowOpenHandler`, which routes a `target=_blank` from the panel to a sandboxed session
// instead of a preload-bearing window.
const electron_1 = require("electron");
const summrise_shell_policy_1 = require("./summrise_shell_policy");
/** The CDP port every session window is advertised on. Read from the SAME boot table `main.ts` parses,
 *  rather than bound: it is a policy constant, not host state — `main.ts` does not own 9333, it passes
 *  it to `--remote-debugging-port` and `agent/summrise-shell-policy`'s `boot.rs` defines it. Bound state
 *  is for what the host OWNS; a second reader of the crate's table is not a second decision. */
const CDP_PORT = JSON.parse((0, summrise_shell_policy_1.shellConstants)()).CDP_PORT;
let bound = null;
/** Called ONCE by `main.ts`, before any `browser-session:*` channel can be invoked. */
function browserSessionsBind(host) { bound = host; }
function deps() {
    // LOUD, not silent: an unbound host would otherwise open a window on a fallback URL, with no icon and
    // no cap — the failures a reader would have to reconstruct from a browser that went to the wrong page.
    if (!bound)
        throw new Error("browser-sessions: browserSessionsBind() was never called — main.ts binds the shell's load door, its window icon and its session cap at startup");
    return bound;
}
const browserSessions = new Map();
/** Initial target per browser window — reuse matching uses this instead of
 *  webContents.getURL() (which lags during navigation; stage-n). */
const browserTargets = new Map();
/** Open a browser-session window on a decided load target (loadTarget() is the
 *  ONE load door — the scheme policy lives there). */
function browserOpen(url) {
    const target = deps().loadTarget(url);
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
    const plan = JSON.parse((0, summrise_shell_policy_1.planBrowserOpen)(JSON.stringify([...browserSessions].map(([id, bw]) => ({ id, target: browserTargets.get(id) ?? "", destroyed: bw.isDestroyed() }))), target, deps().maxBrowserWindows));
    if (plan.reuse) {
        const existing = browserSessions.get(plan.reuse);
        if (existing) {
            if (existing.isMinimized())
                existing.restore();
            existing.show();
            existing.focus();
            return { ok: true, id: plan.reuse, url: target, cdp: (0, summrise_shell_policy_1.cdpEndpoint)(CDP_PORT) };
        }
    }
    if (plan.evict)
        browserClose(plan.evict);
    const id = (0, summrise_shell_policy_1.browserId)(Date.now());
    const bw = new electron_1.BrowserWindow({
        width: 1100, height: 750, title: `Summrise Browser — ${target}`,
        ...(deps().windowIcon() ? { icon: deps().windowIcon() } : {}),
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
    return { ok: true, id, url: target, cdp: (0, summrise_shell_policy_1.cdpEndpoint)(CDP_PORT) };
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
    return { ok: true, sessions: list, cdp: (0, summrise_shell_policy_1.cdpEndpoint)(CDP_PORT) };
}
/** stage-n: close every browser-session window explicitly — app.quit()
 *  tears down the main window but independent BrowserWindows with pending
 *  beforeunload handlers can stall or leak on exit. The table is THIS file's state, so the sweep over it
 *  is too: `main.ts`'s `before-quit` handler calls in, the way it calls the three operations above. */
function browserCloseAll() {
    for (const [id, bw] of browserSessions) {
        if (!bw.isDestroyed())
            bw.destroy();
        browserSessions.delete(id);
        browserTargets.delete(id);
    }
}
