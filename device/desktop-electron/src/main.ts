// Summrise Desktop — the thin Electron host (B2).
//
// RUST OWNS THE DEVICE; THIS FILE OWNS WINDOWS, BOUNDS, THE TRAY AND RETRIES. The device pages,
// data, grouping, counts, tokens and the `/activity`/`/panel` contracts are the Rust shell's. This
// file is view/host glue only, and it holds no page server, no SPA, no shim and no page translation.
//
// TWO WINDOWS, ONE PRIVILEGED FRAME:
//   * the MAIN window loads a Rust device page (`/activity` by default, `/panel` from the tray "Open
//     panel" row) or the static `offline.html` when the device does not answer. It has NO preload, so
//     a device page — or anything a device page navigates to — cannot reach IPC at all.
//   * the HOST window (`hostWin`) is hidden at boot and loads `src/host.html`. It is the ONLY frame
//     with `preload.js`, and it hosts the two native `WebContentsView`s: the browser an AI also
//     drives over CDP :9333, and the harness on this machine's loopback. The tray's "Browser" and
//     "Harness" rows (and the `--browser`/`--harness` launch args) open it.
//
// THE ONE IPC DOOR is `ipcHandle` below: `ipcMain.handle` is called there and nowhere else, and
// `frameOk` admits only the host window's own `webContents` (plus the URL policy's second half). The
// main window and every other frame are refused before a handler runs. `test/ipc-door.test.mjs` pins
// the single-door structure and the channel set against the preload.
//
// THE 9444 LOOPBACK SERVER IS A CONTROL API, NOT A PAGE SERVER (Ruling 8): it keeps
// browser-open/close/list and icon-status; the two agent-task routes died with the wait page.
//
// The policy lives in two Rust crates and is required here as wasm (`summrise_url_policy` for URL/
// origin/port decisions, `summrise_shell_policy` for everything else). Where a decision composes a
// url-policy predicate, THIS file evaluates the predicate and passes its answer in — the two crates
// must not depend on each other, because each glue compiles its own `.wasm` and the port state would
// exist twice.
import { app, BrowserWindow, Tray, Menu, nativeImage, ipcMain, Notification, session, dialog, shell } from "electron";
import * as path from "path";
import * as fs from "fs";
import * as http from "http";

import {
  isBaseOrigin, frameUrlOk, controlOriginOk, sanitizeBrowserUrl, certBypassAllowed, agentBase,
  setAgentPort, parseAgentPort, setDshPort, dshBase, dshOrigins, isDshUrl,
} from "./summrise_url_policy";
import {
  deviceToken, tokenCacheFresh, authorization, resolveAgentPort, resolveDshPort,
  trayIconName, windowIconName, usesAppUserModelId, aumidReport, agentHostLabel, shellConstants,
  forbiddenFrame, cdpEndpoint,
  dshTarget, embeddedPopupTarget, embeddedRecoverUrl, shownUrl, dshPopupTarget, dshHome,
  controlPath, controlQuery, controlIsPreflight, controlRoute, corsAllowOrigin,
  nextRetryMs, shouldRetryLoad, statusIsAlive, tripwireLog,
  cdpUserAgent, cdpUserAgentOurs, cdpSelfCheckOk, cdpWarningForeign, cdpWarningNotResponding,
  cdpWarningUnreachable, refreshTrayHealth, trayShouldWatch,
} from "./summrise_shell_policy";
// THE TWO EMBEDDED VIEWS ARE ONE MODULE, INSTANTIATED TWICE. `embedded-slot.ts` holds the pattern
// (state, place, navigate, reload, recover) and this file builds BOTH instances from it, with the
// differences that are load-bearing passed in as configuration: the load door, the popup decision,
// the recovery target, the state's url fallback, the `gone` channel, and the browser-only nav
// channel. Every `ipcHandle` registration stays HERE, because this file holds the ONE IPC door.
import { embeddedSlot, EmbeddedNavSlot, EmbeddedSlot, SlotBounds } from "./embedded-slot";
// THE BROWSER-SESSION WINDOWS ARE THEIR OWN MODULE. It owns the session table and the three
// operations an AI drives; this file lends it the one load door, the window icon and the cap.
import {
  browserSessionsBind, browserOpen, browserClose, browserList, browserCloseAll,
} from "./browser-sessions";

/** EVERY POLICY NUMBER THIS HOST READS, from `agent/summrise-shell-policy`; `shellConstants()` is the
 *  one parse. A key the host no longer reads (the agent task, the menu flush, the schtasks timeout,
 *  the control probe, the harness-table fetch) is NOT declared here — the crate's table still carries
 *  it, but this interface is the shell's half of the contract, not a copy of the crate's. */
interface ShellConstants {
  CDP_PORT: number; CTRL_PORT: number; MAX_BROWSER_WINDOWS: number;
  RETRY_START_MS: number; TRIPWIRE_BACKOFF_MS: number;
  AGENT_PROBE_BOOT_MS: number; AGENT_PROBE_TRAY_MS: number; AGENT_PROBE_DEFAULT_MS: number;
  STATUS_FETCH_MS: number; CDP_CHECK_MS: number; TRAY_POLL_MS: number;
  CORS_ALLOW_METHODS: string; CORS_ALLOW_HEADERS: string; CORS_VARY: string;
  FORBIDDEN_ORIGIN: string; NOT_FOUND: string;
  ABOUT_BLANK: string; DESKTOP_AUMID: string; AUMID_SET_FAILED: string;
}
const K: ShellConstants = JSON.parse(shellConstants());
// The handful the shell reads often enough that `K.` at every use site would be noise. They are still
// the table's values — destructuring a copy of the parse, not a second declaration.
const {
  AGENT_PROBE_DEFAULT_MS, AGENT_PROBE_BOOT_MS, AGENT_PROBE_TRAY_MS,
  STATUS_FETCH_MS, CDP_CHECK_MS, TRAY_POLL_MS, RETRY_START_MS, TRIPWIRE_BACKOFF_MS,
} = K;

// Layout v2 (ADR 0008): the shell lives at
// <install>\components\summrise-desktop-electron\src\, so the install root is THREE levels up from
// here. All install-root-relative paths derive from this one const.
const INSTALL_ROOT = path.join(__dirname, "..", "..", "..");
let _tokenCache: { at: number; tok: string | null } = { at: 0, tok: null };
function agentToken(): string | null {
  if (tokenCacheFresh(_tokenCache.at, Date.now())) return _tokenCache.tok;
  let raw: string | undefined;
  try {
    raw = fs.readFileSync(path.join(INSTALL_ROOT, "etc", "config.yaml"), "utf8");
  } catch { /* no local config — vitals stay hidden, same as before */ }
  // The pattern is the policy crate's and deliberately NARROWER than the CLI's parser. The failure
  // mode if the issued token ever stops matching is SILENT (no credential, 401, blank vitals), so
  // widen it in the crate before debugging here.
  const tok = deviceToken(raw) ?? null;
  _tokenCache = { at: Date.now(), tok };
  return tok;
}
function authHeaders(): Record<string, string> {
  const value = authorization(agentToken());
  return value ? { authorization: value } : {};
}
// Agent bind port (custom-port installs): explicit SUMMRISE_AGENT_PORT env first, then the agent's
// config.yaml server.port, else the canonical 18080. Resolved once at boot before any probe/window;
// the Rust URL policy's predicates follow via setAgentPort.
import { readDevicePort } from "./device-port";
// TRAY-FIRST: a local app that is already running should not put a window on someone's desktop at
// login. It starts in the notification area; clicking the icon (or a second launch) opens the device.
const START_VISIBLE = process.argv.includes("--show") || process.env.SUMMRISE_START_VISIBLE === "1";
/** `--browser`/`--harness` open the thin host window on that tab (the tray is hidden by Windows 11 by
 *  default and the deploy needs to photograph the panes). A SECOND instance routes them too. `--panel`
 *  is gone: the default page is `/activity`, and the tray's "Open panel" is the door to `/panel`. */
const BOOT_TAB: "browser" | "harness" | null =
  process.argv.includes("--harness") ? "harness" :
  process.argv.includes("--browser") ? "browser" : null;
import { isDevicePage } from "./device-page";
// THE FALLBACK TRIGGER IS ITS OWN MODULE: the path derivation and the detached `--panel` spawn are
// testable without Electron, and this file owns only WHEN to call them (a tray row or `--fallback`).
import { launchFallbackShell } from "./fallback-shell";

function bootAgentPort(): number {
  const fromDevice = readDevicePort();
  if (fromDevice) return fromDevice;
  let fromConfig: number | undefined;
  try {
    fromConfig = parseAgentPort(fs.readFileSync(path.join(INSTALL_ROOT, "etc", "config.yaml"), "utf8")) ?? undefined;
  } catch { /* no local config — default below */ }
  return resolveAgentPort(process.env.SUMMRISE_AGENT_PORT, fromConfig);
}
// Remote-verifiable icon facts (GET /api/shell/icon-status on the 9444 control server): every surface
// records what it resolved so the console can read it back.
const iconReport: Record<string, unknown> = { platform: process.platform };
function statSize(p: string): number {
  try {
    return fs.existsSync(p) ? fs.statSync(p).size : -1;
  } catch {
    return -1;
  }
}
// Brand icon for every native surface: resolve the icon path (existsSync guarded) and record the
// outcome for /api/shell/icon-status.
function resolveIcon(name: string, reportKey: string): string {
  const p = path.join(__dirname, "..", name);
  let out = "";
  try {
    out = fs.existsSync(p) ? p : "";
  } catch {
    out = "";
  }
  iconReport[reportKey] = { path: p, size: statSize(p), used: out !== "" };
  return out;
}
// Tray on Windows needs .ico; BrowserWindow takes .png on ALL platforms (Chromium's ICO parser has
// choked on PNG-compressed 256px entries, silently falling back to the stock electron.exe icon).
function appIcon(): string {
  return resolveIcon(trayIconName(process.platform), "tray");
}
function windowIcon(): string {
  return resolveIcon(windowIconName(), "window");
}
// Native-decode probe: file-exists is NOT proof Electron can use the image. Report what nativeImage
// itself says, so /api/shell/icon-status is ground truth.
function probeImage(p: string): Record<string, unknown> {
  try {
    if (!p) return { empty: true, reason: "no-path" };
    const img = nativeImage.createFromPath(p);
    const sz = img.getSize();
    return { empty: img.isEmpty(), width: sz.width, height: sz.height };
  } catch (e) {
    return { empty: true, reason: String(e).slice(0, 120) };
  }
}

/** The main window: a Rust device page (or the offline page), NO preload. */
let win: BrowserWindow | null = null;
/** The window that holds the narrow preload and the two native views. Hidden at boot; created ONCE
 *  (before the browser slot's eager `ensure()`), reloaded per open, hidden on close. */
let hostWin: BrowserWindow | null = null;
let tray: Tray | null = null;
/** The last tab the host page was asked for (`browser` at boot). */
let hostTab: "browser" | "harness" = "browser";
/** `ready-to-show` fires once per window; a second open must SHOW, not wait for an event that will
 *  never fire again. */
let hostReady = false;
/** The main window's requested device page. The tray's transcript/panel rows set it; the offline
 *  page carries it in its query and the eventual load uses it. */
type DevicePage = "activity" | "panel";
let desiredPage: DevicePage = "activity";
const hostPageFile = path.join(__dirname, "host.html");
const offlinePageFile = path.join(__dirname, "offline.html");
const offlinePageName = path.basename(offlinePageFile);

/** The device's operations transcript or panel on the agent's actual origin. */
function devicePageUrl(): string {
  return new URL(`/${desiredPage}`, agentBase()).toString();
}
/** The device's own pages (`/activity`, `/panel`) on the agent's origin — that is, pages this window
 *  may keep. */
function onDevicePage(url: string): boolean {
  return isBaseOrigin(url) && isDevicePage(url, agentBase());
}
/** Whether a URL is the static offline file (with or without its `?address=` query). Matched by the
 *  parsed file name rather than an exact string prefix: the same file is spelled `file:///C:/…` on
 *  Windows and `file:///home/…` here, and a drive-letter/encoding difference must not make the
 *  tripwire treat the offline page as foreign. */
function isOfflinePage(url: string): boolean {
  let parsed: URL;
  try { parsed = new URL(url); } catch { return false; }
  if (parsed.protocol !== "file:") return false;
  const base = parsed.pathname.slice(parsed.pathname.lastIndexOf("/") + 1);
  try { return decodeURIComponent(base) === offlinePageName; } catch { return base === offlinePageName; }
}

// IPC audit #2: the preload is only attached to the host window, but the frame check stays explicit
// and is applied ONCE, in the door. The main window (a device page, no preload) and every other frame
// fail; the host page's own `webContents` passes by SENDER, with `frameUrlOk` the URL-policy second
// half. The sender half is not optional: a `file://` host page is not a device URL, so `frameUrlOk`
// alone refuses the exact URL the app loads (`test/ipc-door.test.mjs` pins the two uses).
function frameOk(e: Electron.IpcMainInvokeEvent): boolean {
  return (!!hostWin && !hostWin.isDestroyed() && e.sender === hostWin.webContents) || frameUrlOk(e.senderFrame?.url || "");
}
// THE ONE REFUSAL a forbidden frame gets. The value is the policy crate's, so the one refusal has one
// owner; `Object.freeze` is this file's guarantee to itself.
const FORBIDDEN_FRAME: Readonly<{ ok: false; error: string }> = Object.freeze(JSON.parse(forbiddenFrame()));
// THE ONE IPC DOOR: `ipcMain.handle` is called HERE and nowhere else, so the frame check above is
// applied ONCE per channel and no handler can forget it. A handler that registered itself elsewhere
// would be silent (main.ts imports electron, so no node suite can import it) — the pin is the source
// check in test/ipc-door.test.mjs. Handlers get the invoke ARGUMENTS only.
function ipcHandle<A extends unknown[], R>(channel: string, fn: (...args: A) => R): void {
  ipcMain.handle(channel, (e: Electron.IpcMainInvokeEvent, ...args: A) => {
    if (!frameOk(e)) return FORBIDDEN_FRAME;
    return fn(...args);
  });
}
/** Whether the hide-to-tray notification has been shown (once per launch). */
let hideNotified = false;

/** Build (or rebuild) the host window. The ONLY window with a preload and the only frame `frameOk`
 *  accepts by sender. */
function createHostWindow(): BrowserWindow {
  hostReady = false;
  const host = new BrowserWindow({
    width: 1200,
    height: 800,
    title: "Summrise Browser",
    show: false,
    backgroundColor: "#111",
    ...(windowIcon() ? { icon: windowIcon() } : {}),
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
      // Defense in depth: the preload is static/safe, but sandbox removes any Node escape path.
      sandbox: true,
      // The panes must paint while the window is hidden or behind another app.
      backgroundThrottling: false,
    },
  });
  // THE HOST DOCUMENT IS ONE LOCAL FILE. A navigation away from it is never the point and a popup
  // would be an unmanaged window; both are refused.
  host.webContents.on("will-navigate", (event) => { event.preventDefault(); });
  host.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
  // The title is the shell's; a page title must not replace "Summrise Browser".
  host.on("page-title-updated", (event) => { event.preventDefault(); });
  // CLOSE HIDES TO THE TRAY (quit destroys): the tray's Browser/Harness rows bring it back.
  host.on("close", (event) => {
    if (!(app as any).isQuitting) {
      event.preventDefault();
      host.hide();
    }
  });
  // A crashed host renderer cannot drive its views any more: hide them rather than leave a native
  // page floating over a dead document. The next open reloads the host page, which re-places them.
  host.webContents.on("render-process-gone", () => {
    if ((app as any).isQuitting) return;
    try { browserSlot.place(null); } catch { /* the view may already be gone */ }
    // ONLY AN EXISTING VIEW. `place(null)` calls `ensure()`, and the harness slot is LAZY: hiding a
    // view that was never built would CREATE it at about:blank, and the next Harness open would read
    // that record as an already-started pane. `state()` reads the record without creating anything.
    try { if (dshSlot.state().visible) dshSlot.place(null); } catch { /* the view may already be gone */ }
  });
  host.once("ready-to-show", () => { hostReady = true; });
  return host;
}

/** Open the host window on a tab: create it if needed, reload it on that tab, then show/focus. The
 *  side effect is the contract: Browser and Harness are reachable even when Windows 11 hides the
 *  tray icon, and the deploy can photograph either pane. */
function openHost(tab: "browser" | "harness"): void {
  if (!app.isReady()) return;
  hostTab = tab;
  if (!hostWin || hostWin.isDestroyed()) hostWin = createHostWindow();
  void hostWin.loadFile(hostPageFile, { query: { tab } }).catch(() => { /* the local host page */ });
  const reveal = (): void => {
    if (!hostWin || hostWin.isDestroyed()) return;
    hostWin.setTitle("Summrise Browser");
    hostWin.show();
    hostWin.focus();
  };
  if (hostReady) reveal();
  else hostWin.once("ready-to-show", reveal);
}

function focusMain(): void {
  if (!win || win.isDestroyed()) return;
  if (win.isMinimized()) win.restore();
  win.show();
  win.focus();
}

/** Open the installed Rust shell in its WINDOW-ONLY mode (`--panel`): no tray, helper, first-run,
 *  geometry or reconnect. Called from the tray row and from an explicit `--fallback` argv only — this
 *  host never starts the fallback by itself, because Electron stays the one automatic surface.
 *
 *  The path is `process.execPath`'s sibling (the installed `summrise-shell.exe`), not a config value.
 *  A missing shell is a visible dialog rather than a dead click; a spawn failure is the same door. */
function openFallbackShell(): void {
  const result = launchFallbackShell({ execPath: process.execPath });
  if (result.ok) {
    console.log(`[summrise] fallback shell opened: ${result.path}${result.pid ? ` (pid ${result.pid})` : ""}`);
    return;
  }
  dialog.showErrorBox(
    "Summrise fallback shell",
    "The window-only fallback shell (summrise-shell.exe) could not be opened.\n\n" + result.error,
  );
}

// stage-m: SINGLE-INSTANCE LOCK — a second Summrise exits, and its argv is routed to the running
// instance: `--browser`/`--harness` open the host window, anything else focuses the main window.
const gotTheLock = app.requestSingleInstanceLock();
if (!gotTheLock) {
  app.quit();
} else {
  app.on("second-instance", (_event, argv) => {
    if (argv.includes("--fallback")) openFallbackShell();
    else if (argv.includes("--harness")) openHost("harness");
    else if (argv.includes("--browser")) openHost("browser");
    else focusMain();
  });
}

// CDP must be enabled before app ready — pass it through Chromium switches.
app.commandLine.appendSwitch("remote-debugging-port", String(K.CDP_PORT));
// Lab-device self-signed certs (OpenWrt-style ONT defaults): bypass cert errors ONLY on
// private-network hosts — the public internet keeps full validation. Registered before ready so no
// navigation can race it.
app.on("certificate-error", (event, _webContents, url, _error, _certificate, callback) => {
  if (certBypassAllowed(String(url || ""))) {
    event.preventDefault();
    callback(true);
    return;
  }
  callback(false);
});
// Hidden windows: Chromium flips visibilityState to "hidden" and stops requestAnimationFrame, so the
// native views' pages stop painting. backgroundThrottling:false alone does NOT restore rAF for hidden
// pages; these renderer-level switches do.
app.commandLine.appendSwitch("disable-renderer-backgrounding");
app.commandLine.appendSwitch("disable-backgrounding-occluded-windows");

/** Tell a first run how Windows 11 hides new tray icons, and offer the one settings page that fixes
 *  it. The hint is written to `userData`, so it is shown once per profile, not once per launch. */
let trayHintPending = false;

function offerTraySettingsOnce(): void {
  try {
    const marker = path.join(app.getPath("userData"), "tray-settings-shown");
    if (fs.existsSync(marker)) return;
    // NOT A MODAL OVER SOMEONE ELSE'S FULL-SCREEN APP: the dialog waits for the Summrise window
    // itself to be focused. The marker is written only when the hint is actually shown.
    trayHintPending = true;
  } catch { /* the hint is a courtesy; a machine that refuses it still has a working tray */ }
}

function showTrayHintIfPending(): void {
  if (!trayHintPending) return;
  trayHintPending = false;
  try {
    const marker = path.join(app.getPath("userData"), "tray-settings-shown");
    fs.mkdirSync(path.dirname(marker), { recursive: true });
    fs.writeFileSync(marker, new Date().toISOString());
    void dialog.showMessageBox({
      type: "info",
      title: "Summrise tray icon",
      message: "If you do not see the Summrise icon in the notification area",
      detail: "Windows 11 hides new tray icons by default. Open Settings → Personalization → Taskbar → Other system tray icons and turn on Summrise.",
      buttons: ["Open settings", "Later"],
      defaultId: 0,
      cancelId: 1,
    }).then((choice) => {
      if (choice.response === 0) void shell.openExternal("ms-settings:taskbar");
    });
  } catch { /* the hint is a courtesy; a machine that refuses it still has a working tray */ }
}

// --- Browser-session control: shared core (used by both IPC and HTTP) ---
/** THE ONE LOAD DOOR (stage-n hardening): every raw URL this shell is asked to load — a
 *  browser-session window, the embedded browser view, a `window.open()` from a page inside it — is
 *  decided HERE and nowhere else.
 *
 *  Only http/https/about:blank survive; file:// and every other scheme collapse to "about:blank",
 *  because a loadable file:// would hand the AI a local-file read primitive via the shared CDP
 *  endpoint (:9333). The DECISION itself is the Rust policy's `sanitizeBrowserUrl()`; this function
 *  is that predicate's ONE call site in the shell (`test/ipc-door.test.mjs` pins the count), so a
 *  new load site cannot quietly reach for a weaker policy of its own. */
function loadTarget(raw: string | undefined): string {
  return sanitizeBrowserUrl(raw);
}
// ── THE BROWSER SLOT: THE FIRST INSTANCE OF `embedded-slot.ts` ─────────────────────────────────
// Everything the slot needs is either a function object this file owns (the window getter and the
// one load door) or a policy answer only THIS view may use — the popup decision and the recovery
// target keep this view a general browser while the harness's is confined to its own loopback origin.
const browserSlot: EmbeddedNavSlot = embeddedSlot({
  // A GETTER, not the value: the window does not exist yet and is nulled when it closes.
  win: () => hostWin,
  door: loadTarget,
  // `target=_blank` through the ONE load door: its argument is the DECIDED target, and a refused
  // scheme answers `undefined` — DROPPED rather than collapsed to `about:blank`, so an external link
  // cannot erase the page the operator is reading.
  popupTarget: (raw) => embeddedPopupTarget(loadTarget(raw)),
  // The last DECIDED url, or the crate's blank fallback: recovering a crashed view must not fetch a
  // third-party home page to do it.
  recoverUrl: (lastDecided) => embeddedRecoverUrl(lastDecided),
  // This state feeds the host page's address bar, which must never go blank: a view that cannot
  // answer reports the last url the door decided for it. (The harness's record does the opposite.)
  stateUrl: (live, lastDecided) => shownUrl(live, lastDecided),
  goneChannel: "embedded-browser:gone",
  // THE BROWSER-ONLY SURFACE: nav pushes, canBack/canFwd, and go/zoom. The harness instance has none.
  navChannel: "embedded-browser:nav",
});
// The browser-session windows' seam, bound ONCE here. Three things this file owns and the sessions do
// not: the ONE load door above, the window icon, and the cap's number.
browserSessionsBind({ loadTarget, windowIcon, maxBrowserWindows: K.MAX_BROWSER_WINDOWS });

// ── THE EMBEDDED VIEWS' IPC — THE REGISTRATIONS STAY HERE ──────────────────────────────────────
// The slot's state and operations are `embedded-slot.ts`. These lines do NOT go with it: this file
// holds the ONE `ipcMain.handle` door, and `test/ipc-door.test.mjs` pins the channels registered
// behind that door against the ones `preload.ts` invokes. A handler registered in the module would be
// a second door.
ipcHandle("embedded-browser:recover", () => browserSlot.recover());
ipcHandle("embedded-browser:navigate", (url: string) => browserSlot.navigate(String(url || "")));
ipcHandle("embedded-browser:back", () => browserSlot.go(-1));
ipcHandle("embedded-browser:fwd", () => browserSlot.go(1));
ipcHandle("embedded-browser:reload", () => browserSlot.reload());
ipcHandle("embedded-browser:zoom", (factor: number) => browserSlot.zoom(factor));
ipcHandle("embedded-browser:place", (bounds: SlotBounds | null) => {
  browserSlot.place(bounds);
  return { ok: true };
});
ipcHandle("embedded-browser:state", () => browserSlot.state());

// ── THE HARNESS VIEW: the harness's own UI, embedded in Summrise ───────────────────────────────
//
// WHY LOOPBACK AND NOT THE PUBLIC HOSTNAME. The harness runs on THIS machine (127.0.0.1:18081), and
// this view is a window ON this machine — so loopback is the correct address, not the remote one.
// Going out to the tunnel hostname instead would mean a Cloudflare round trip for every asset, an
// Access login inside the shell, and a certificate story — for a service that is already here.
//
// A SECOND view, not a re-use of the browser one: the browser view is a single-tab internet browser
// an AI drives over CDP, and navigating it to the harness would throw that session away.
/** The harness's port. Env first (a deployment may move it), else the component's default — the range
 *  test and the fallback are the policy crate's, the env read is this file's. */
function bootDshPort(): number {
  return resolveDshPort(process.env.SUMMRISE_DSH_PORT);
}
/** The ONE load door for this view: its own origin, or nothing. `isDshUrl` is the URL policy's
 *  predicate over its own port list, and this file evaluates it because that list is state the two
 *  wasm modules must not each hold a copy of. The serialization of an admitted target — and the
 *  refusal of everything else — is the shell policy's. */
function dshTargetFor(raw: string): string {
  return dshTarget(raw, isDshUrl(raw));
}
// THE HARNESS INSTANCE, built ONCE here from the SAME factory as the browser slot. The window is a
// getter because it does not exist yet; `door` is the load door above. The LIFECYCLE IS THE CALLER'S,
// NOT A PARAMETER: this slot is created LAZILY (nothing calls `ensure()` until the pane's `open()`
// navigates), where the browser's is created eagerly at startup.
const dshSlot: EmbeddedSlot = embeddedSlot({
  win: () => hostWin,
  door: dshTargetFor,
  // Over its own door, and only the harness's origin is admitted — an external `target=_blank` is
  // dropped rather than navigating this pane away from the harness.
  popupTarget: (raw) => dshPopupTarget(raw, isDshUrl(raw)),
  // Always the harness home: a crash recovery that lands somewhere other than the harness is a view
  // the operator did not ask for.
  recoverUrl: () => dshHome(dshBase()),
  // This record describes the LIVE view and nothing else — a dead webContents reports blank rather
  // than its last url (the browser slot's address bar is the opposite case).
  stateUrl: (live) => shownUrl(live, ""),
  goneChannel: "embedded-dsh:gone",
  // NO navChannel: the harness pane has no address bar and no back/fwd buttons.
});
/** Open (or re-focus) the harness at its configured address. Idempotent — and the `open` OPERATION
 *  stays in this file because the lifecycle is the caller's: this is the call that CREATES the
 *  harness's view (the slot builds it lazily inside `navigate`), through the ONE door above. */
function dshOpen(): { ok: boolean; url: string } {
  return dshSlot.navigate(dshHome(dshBase()));
}
ipcHandle("embedded-dsh:open", () => dshOpen());
ipcHandle("embedded-dsh:place", (bounds: SlotBounds | null) => {
  dshSlot.place(bounds);
  return { ok: true };
});
ipcHandle("embedded-dsh:state", () => dshSlot.state());
// SELECT A HOST: the slot's `navigate` already refuses anything `isDshUrl` does not admit, so this
// channel can only reach a door the policy configured — and it answers with the address it actually
// loaded, which for a refusal is `about:blank`.
ipcHandle("embedded-dsh:go", (url) => dshSlot.navigate(String(url)));
ipcHandle("embedded-dsh:reload", () => dshSlot.reload());
ipcHandle("embedded-dsh:recover", () => dshSlot.recover());
// THE HARNESS SELECTOR'S DOORS. `dshOrigins()` is the URL policy's own list (the default harness
// origin plus any door the policy was told about); the device serves no `/api/workspace/harnesses`
// route, so this is the whole table — never a list this shell keeps a second copy of.
ipcHandle("embedded-dsh:targets", () => ({ ok: true, targets: dshOrigins() }));

// --- Agent lifecycle (stage-m: Rust owns it; the shell only probes) ---
// review #2 (HIGH): a raw TCP connect only proves the PORT is held — an agent that is wedged but
// still listening keeps the connect succeeding. Liveness = GET /api/health answers within the budget.
function agentResponds(timeoutMs = AGENT_PROBE_DEFAULT_MS): Promise<boolean> {
  return new Promise((res) => {
    let done = false;
    const finish = (v: boolean) => { if (!done) { done = true; res(v); } };
    const req = http.get(`${agentBase()}/api/health`, { timeout: timeoutMs }, (r) => {
      r.resume();
      // ANY HTTP response = the accept loop is alive (this is the exact thing the TCP probe could not
      // see). The health route needs no token, so its normal 200 and its degraded 5xx both count;
      // only no answer at all (timeout / connection error) is dead.
      finish(statusIsAlive(r.statusCode));
    });
    req.on("timeout", () => { req.destroy(); finish(false); });
    req.on("error", () => finish(false));
  });
}

// THE 9444 LOOPBACK CONTROL API (Ruling 8): browser-open/close/list and icon-status. A JSON control
// surface for native tooling and the CDP/deploy path — NOT a page server (the shim that served the
// SPA is gone), and no agent task routes (the wait page that called them is gone).
const httpServer = http.createServer((req, res) => {
  // THE REQUEST PARSE AND THE ROUTER ARE THE POLICY CRATE'S. `controlPath` resolves the request target
  // against the loopback base the way `new URL(req.url, "http://127.0.0.1")` did, and a malformed
  // request target answers `undefined` instead of throwing out of this listener.
  const pathname = controlPath(req.url) ?? "";
  const send = (obj: unknown, code = 200) => {
    // review #8: was ACAO:* with no origin check — ANY web page open in ANY local browser could POST
    // /api/browser-session/open (loopback is exempt from mixed-content blocking). Reflect the origin,
    // and reject requests from foreign origins (reads included — the GET routes expose session URLs
    // and device facts). "null" and absent (native tooling/curl) stay allowed.
    res.setHeader("access-control-allow-origin", corsAllowOrigin(req.headers.origin));
    res.setHeader("access-control-allow-methods", K.CORS_ALLOW_METHODS);
    res.setHeader("access-control-allow-headers", K.CORS_ALLOW_HEADERS);
    res.setHeader("vary", K.CORS_VARY);
    res.writeHead(code, { "content-type": "application/json", "cache-control": "no-store" });
    res.end(JSON.stringify(obj));
  };
  // A PREFLIGHT IS ANSWERED BEFORE THE VETO: moving the check below would start 403-ing legitimate
  // CORS preflights and the plain-browser path would stop working with no other symptom.
  if (controlIsPreflight(req.method ?? "")) return send({ ok: true });
  // THE DECISION ITSELF IS RUST (agent/summrise-url-policy) and was an inline regex with NO `$`
  // ANCHOR, so `http://127.0.0.1.evil.com` passed — the same class as the `startsWith(BASE)` bug that
  // crate exists to kill.
  if (!controlOriginOk(req.headers.origin)) {
    return send({ ok: false, error: K.FORBIDDEN_ORIGIN }, 403);
  }
  try {
    // THE ROUTE TABLE IS THE POLICY CRATE'S, INCLUDING ITS ASYMMETRY, which is the original behaviour
    // and not an accident to tidy: `/list` and `/icon-status` have NO method guard (a POST reaches the
    // same reader), while `/open` and `/close` require POST and a GET to one of them falls through to
    // the 404. The agent-task routes are not in the surviving set any more: they fall through to the
    // same 404.
    switch (controlRoute(req.method ?? "", pathname)) {
      case "browser-open": return send(browserOpen(controlQuery(req.url, "url") || K.ABOUT_BLANK));
      case "browser-close": return send(browserClose(controlQuery(req.url, "id") || ""));
      case "browser-list": return send(browserList());
      case "icon-status": return send({ ok: true, icons: iconReport,
        native: {
          tray: probeImage(String((iconReport["tray"] as any)?.path || "")),
          window: probeImage(String((iconReport["window"] as any)?.path || "")),
        } });
      default: send({ ok: false, error: K.NOT_FOUND }, 404);
    }
  } catch (e) {
    send({ ok: false, error: String(e) }, 500);
  }
});

if (gotTheLock) {
  // TASKBAR GROUPING AND THE TASKBAR GROUP'S ICON — HALF ONE OF TWO, AND THIS IS THE HALF THAT IS A
  // PRECONDITION RATHER THAN A HINT. Microsoft's rule for System.AppUserModel.RelaunchIconResource is
  // a three-step lookup (window property; else a shortcut with the same ID; else the backing
  // executable). Which step is reachable at all depends on THIS call. The value, the platform test
  // and the report strings are the policy crate's; the shortcut half is written by the CLI, and
  // `agent/tests/shared_literals.rs` compares the two from both sources.
  try {
    if (usesAppUserModelId(process.platform)) app.setAppUserModelId(K.DESKTOP_AUMID);
    iconReport["appUserModelId"] = aumidReport(process.platform);
  } catch {
    iconReport["appUserModelId"] = K.AUMID_SET_FAILED;
  }
  app.whenReady().then(async () => {
    // Custom-port installs: pin every origin predicate + probe/load URL to the agent's actual bind
    // port BEFORE any window or probe exists.
    setAgentPort(bootAgentPort());
    // The harness view's door, pinned the same way and for the same reason: a predicate that ran
    // before this line would check a port nothing is listening on.
    setDshPort(bootDshPort());

    // `--fallback` is the explicit argv door to the installed Rust shell, checked ONCE here and never
    // derived from the boot state. It runs BEFORE the host windows so an explicit fallback does not
    // wait on (or depend on) this host's device-page load; a normal start never reaches it.
    if (process.argv.includes("--fallback")) openFallbackShell();

    // ── THE HOST WINDOW: created ONCE, after the ports are pinned and BEFORE the browser slot's
    // eager `ensure()` (the slot's window getter is this window, and a view created before it would
    // have nothing to attach to). It stays hidden until the tray's Browser/Harness rows, a second
    // instance, or `--browser`/`--harness` ask for it. The null check is not decoration: a
    // `second-instance` that lands in the same tick as `ready` can call `openHost` first, and the
    // boot must adopt that window rather than orphan it under a second one.
    if (!hostWin || hostWin.isDestroyed()) {
      hostWin = createHostWindow();
      void hostWin.loadFile(hostPageFile, { query: { tab: hostTab } }).catch(() => { /* the local host page */ });
    }

    // NO NATIVE APPLICATION MENU. Removing the old menu template alone would leave Electron's default
    // File/Edit/View/Window/Help menu; the tray's context menu is separate (`Menu.buildFromTemplate`).
    Menu.setApplicationMenu(null);

    // review #7: with no handler Electron AUTO-GRANTS every permission request (media/geolocation/
    // clipboard) — deny by default for all windows, especially the remote-browser ones.
    try {
      session.defaultSession.setPermissionRequestHandler((_wc, _perm, cb) => cb(false));
      session.defaultSession.setPermissionCheckHandler(() => false);
    } catch { /* non-fatal */ }
    httpServer.listen(K.CTRL_PORT, "127.0.0.1");
    console.log(`[summrise] browser-session control: http://127.0.0.1:${K.CTRL_PORT}`);

    // ── THE MAIN WINDOW: a DEVICE PAGE, and NO PRELOAD. This is the B1 review item: the main window
    // loads the Rust page, so if it carried the preload that page could drive the host. It does not.
    win = new BrowserWindow({
      width: 1200, height: 800, title: "Summrise",
      // NO WHITE FLASH: created hidden, shown on the first paint. backgroundColor paints that first
      // frame in the shell's own ink instead of Chromium's white.
      show: false,
      backgroundColor: "#111",
      // Taskbar + title-bar icon: without this Windows shows the stock electron.exe icon. PNG —
      // Chromium's ICO parser has choked on PNG-compressed 256px entries.
      ...(windowIcon() ? { icon: windowIcon() } : {}),
      webPreferences: {
        // NO PRELOAD. The only frame that may invoke IPC is `hostWin`; a device page must not be
        // able to reach the host at all (the B1 review item).
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: true,
        // Hidden windows keep painting (the device's own UI may use rAF).
        backgroundThrottling: false,
      },
    });
    // The other half of show:false — the first paint shows the window. Registered in the same tick as
    // the constructor, before any load can happen, so there is no window in which the event is missed.
    win.once("ready-to-show", () => { if (START_VISIBLE) win?.show(); });
    win.on("focus", () => { showTrayHintIfPending(); });
    // A device page may link within its own origin (the offline file is the other admitted document);
    // anything else is refused. The main window has no bridge, so this is defense in depth — the
    // tripwire below catches CDP-driven navigation, which bypasses will-navigate.
    win.webContents.on("will-navigate", (e, url) => {
      if (!isBaseOrigin(url) && !isOfflinePage(url)) e.preventDefault();
    });
    // round-258 (device-caught): CDP-driven navigation (an attached playwright/AI) BYPASSES
    // will-navigate — a browser_navigate call hijacked the main window and the panel vanished.
    // Tripwire: the moment the MAIN window lands anywhere that is not a device page, the offline page
    // or about:blank, snap it straight back to the requested device page.
    let snappingBack = false;
    win.webContents.on("did-navigate", (_e, url) => {
      if (snappingBack) return;
      if (isDevicePage(url, agentBase()) || isOfflinePage(url) || url === K.ABOUT_BLANK) return;
      console.log(tripwireLog(url));
      snappingBack = true;
      win?.loadURL(devicePageUrl()).catch(() => { /* retry below */ }).finally(() => {
        setTimeout(() => { snappingBack = false; }, TRIPWIRE_BACKOFF_MS);
      });
    });
    // A target=_blank from a device page must NOT get a preload-bearing window; route it to a
    // sandboxed browser session instead.
    win.webContents.setWindowOpenHandler(({ url }) => {
      browserOpen(url);
      return { action: "deny" };
    });

    // ── THE DEVICE-PAGE LOAD LOOP ────────────────────────────────────────────────────────────────
    // `loadDesktop` probes HTTP liveness (agentResponds), not a raw TCP connect: a wedged-but-
    // listening agent holds the port yet never answers. Alive → the requested device page. Down →
    // the static offline page plus a policy-owned backoff (`nextRetryMs`, capped) until it answers.
    let retryMs = RETRY_START_MS;
    let retryTimer: NodeJS.Timeout | null = null;
    const resetRetry = (): void => { retryMs = RETRY_START_MS; };
    const nextRetry = (): number => { retryMs = nextRetryMs(retryMs); return retryMs; };
    const cancelRetry = (): void => {
      if (retryTimer) { clearTimeout(retryTimer); retryTimer = null; }
    };
    const loadDevicePage = (): void => {
      cancelRetry();
      void win?.loadURL(devicePageUrl()).catch(() => { /* did-fail-load retries below */ });
    };
    const loadOfflinePage = (force = false): void => {
      // DO NOT RE-LOAD A PAGE THAT IS ALREADY SHOWING: the automatic retry is the backoff above and
      // the Retry button is the manual reload. A reload every 2-30 s would flicker and wipe what the
      // page says it observed (the old wait page carried the same guard). `force` is for the ONE case
      // where the page is stale rather than being retried: the tray asked for the OTHER device page
      // while offline, and the offline page names the page it will open.
      if (!force && isOfflinePage(win?.webContents.getURL() ?? "")) return;
      void win?.loadFile(offlinePageFile, { query: { address: agentHostLabel(agentBase()), page: desiredPage } })
        .catch(() => { /* the retry loop below is the recovery */ });
    };
    const scheduleRetry = (): void => {
      cancelRetry();
      retryTimer = setTimeout(() => { retryTimer = null; void loadDesktop(); }, nextRetry());
    };
    const loadDesktop = async (): Promise<void> => {
      if (await agentResponds(AGENT_PROBE_BOOT_MS)) {
        resetRetry();
        loadDevicePage();
        return;
      }
      loadOfflinePage();
      scheduleRetry();
    };
    /** The tray's transcript/panel rows: remember the requested page, bring the window forward, and
     *  probe/reload through the one loop. If the offline page is showing and the requested page
     *  CHANGED, reload that page once so its label names the page that will open — otherwise the
     *  operator reads the old page name until the device answers. */
    const openDevicePage = (page: DevicePage): void => {
      const changed = page !== desiredPage;
      desiredPage = page;
      focusMain();
      if (changed && isOfflinePage(win?.webContents.getURL() ?? "")) {
        loadOfflinePage(true);
        return;
      }
      void loadDesktop();
    };
    // Retry loop: did-fail-load (the device went down mid-load) re-runs loadDesktop; each failure
    // schedules exactly one retry. `shouldRetryLoad` excludes ERR_ABORTED and subframe failures,
    // because a same-URL reload dispatches did-fail-load(-3) on the MAIN frame and subframe failures
    // fired too — either would stack a second chain with accelerated retry counting.
    win.webContents.on("did-fail-load", (_e, errorCode, _desc, _url, isMainFrame) => {
      if (!shouldRetryLoad(!!isMainFrame, errorCode)) return;
      scheduleRetry();
    });
    // The offline page's own load is ONE more probe (the Retry button reloads it, and the automatic
    // backoff is already running). If the device answered in the meantime, go straight to the page.
    win.webContents.on("did-finish-load", () => {
      if (!isOfflinePage(win?.webContents.getURL() ?? "")) return;
      void agentResponds(AGENT_PROBE_BOOT_MS).then((alive) => {
        if (!alive || !win || win.isDestroyed()) return;
        resetRetry();
        loadDevicePage();
      });
    });
    await loadDesktop();
    // round-259: eagerly create the browser view at startup (hidden) so an attached playwright/AI
    // ALWAYS has its own dedicated page to drive. NO EAGER NAVIGATION: it used to load bing.com on
    // every launch. about:blank is a first-class CDP target, and the user/AI navigates it next
    // (`loadTarget` is the one door). This is the BROWSER slot's lifecycle and it lives here on
    // purpose: the browser's view is created eagerly, the harness's is created lazily by its pane.
    try {
      browserSlot.ensure();
    } catch { /* non-fatal */ }
    console.log(`[summrise] CDP endpoint: ${cdpEndpoint(K.CDP_PORT)} (playwright connectOverCDP)`);
    // stage-n: CDP self-check — if 9333 is occupied by another process (a second browser/electron),
    // remote-debugging-port silently fails and AI driving would hit the WRONG target. Probe
    // /json/version and verify the User-Agent belongs to this app.
    (async () => {
      try {
        const ctrl = new AbortController();
        const t = setTimeout(() => ctrl.abort(), CDP_CHECK_MS);
        const r = await fetch(`http://127.0.0.1:${K.CDP_PORT}/json/version`, { signal: ctrl.signal });
        clearTimeout(t);
        if (r.ok) {
          const ua = cdpUserAgent(await r.text());
          if (cdpUserAgentOurs(ua)) {
            console.log(cdpSelfCheckOk(K.CDP_PORT));
          } else {
            console.warn(cdpWarningForeign(K.CDP_PORT, ua));
          }
        } else {
          console.warn(cdpWarningNotResponding(K.CDP_PORT));
        }
      } catch {
        console.warn(cdpWarningUnreachable(K.CDP_PORT));
      }
    })();
    // The `--browser`/`--harness` launch args open the host window once the browser view exists, so
    // the Browser tab is not an empty pane while the slot is still being ensured.
    if (BOOT_TAB) openHost(BOOT_TAB);
    win.on("close", (e) => {
      if (!(app as any).isQuitting) {
        e.preventDefault();
        win?.hide();
        // stage-n: first close hides to tray — tell the user once so they don't think the app
        // exited (a silent hide reads as 'closed').
        if (!hideNotified) {
          hideNotified = true;
          try {
            new Notification({ title: "Summrise", body: "Summrise is still running in the system tray." }).show();
          } catch { /* notifications unavailable — harmless */ }
        }
      }
    });
    // Windows requires .ico for tray; macOS/Linux accept .png.
    const iconPath = appIcon();
    tray = new Tray(iconPath ? iconPath : nativeImage.createEmpty());
    tray.setToolTip("Summrise");
    console.log(`[summrise] icons ${JSON.stringify(iconReport)}`);
    offerTraySettingsOnce();
    tray.on("click", () => focusMain());
    // stage-n: tray reflects live agent state — tooltip + a status line in the menu, refreshed on a
    // 30s poll and on demand.
    // stage-n: guards the single loadDesktop re-entry loop (see tray hook).
    let agentWatchActive = false;
    /** WHAT THE TRAY KNOWS, carried between polls: the facts the last `/api/status` answer left
     *  behind. The RULE — a poll that fails, or an answer that omits a field, KEEPS what the last one
     *  said — is the policy crate's; the STATE is this file's, because a poll is the only thing that
     *  observes it. */
    let trayFacts: { version: string; uptime: string; sessions: number; cpu: number | null; mem: number | null } =
      { version: "", uptime: "", sessions: 0, cpu: null, mem: null };
    /** WHEN /api/status last ANSWERED (null = it never has since launch) — the tray's tooltip reports
     *  this instead of asserting a state it cannot date. */
    let trayLastAnsweredAt: number | null = null;
    const refreshTray = async (): Promise<void> => {
      // HTTP liveness, not the TCP probe (see agentResponds above): a wedged-but-listening agent must
      // show as STOPPED, not "running".
      const running = await agentResponds(AGENT_PROBE_TRAY_MS);
      const observedAt = Date.now();
      // THE PARSE, THE KEEP-LAST RULE, THE VITALS LINE AND THE TOOLTIP'S WORDING ARE ONE CALL INTO THE
      // POLICY CRATE, and this file hands over the RAW BODY rather than a parsed object: `r.json()`
      // throwing into a `catch { keep last }` IS the keep-last rule, so the parse belongs where the
      // rule is.
      let statusText = "";
      if (running) {
        try {
          const ctrl = new AbortController();
          const t = setTimeout(() => ctrl.abort(), STATUS_FETCH_MS);
          const r = await fetch(`${agentBase()}/api/status`, { signal: ctrl.signal, headers: authHeaders() });
          clearTimeout(t);
          if (r.ok) statusText = await r.text();
        } catch { /* keep last */ }
      }
      // THE LOCAL TIME ZONE IS A HOST FACT AND IT IS PASSED PER INSTANT: wasm has no clock and no zone
      // database, and the two clock faces the tooltip can print are up to a DST boundary apart.
      const next = JSON.parse(refreshTrayHealth(JSON.stringify({
        running,
        statusText,
        prev: trayFacts,
        observedAt,
        lastAnsweredAt: trayLastAnsweredAt,
        tzOffsetMin: new Date(observedAt).getTimezoneOffset(),
        lastTzOffsetMin: new Date(trayLastAnsweredAt ?? observedAt).getTimezoneOffset(),
      }))) as {
        facts: typeof trayFacts;
        health: string;
        lastAnsweredAt: number | null;
      };
      trayFacts = next.facts;
      trayLastAnsweredAt = next.lastAnsweredAt;
      const health = next.health;
      // The agent died WHILE the main window was showing a device page: re-enter the load loop so the
      // window swaps to the offline page (and the automatic retry takes over). One loop guard so
      // repeated tray polls cannot stack retries. The CONJUNCTION is the policy crate's; the ORIGIN
      // half is this file's `onDevicePage`, evaluated only when the window is live.
      const liveWindow = win && !win.isDestroyed() ? win : null;
      if (trayShouldWatch(running, agentWatchActive, liveWindow !== null,
          liveWindow ? onDevicePage(liveWindow.webContents.getURL()) : false)) {
        agentWatchActive = true;
        void loadDesktop();
      }
      if (running && agentWatchActive) agentWatchActive = false;
      // THE TOOLTIP NAMES THE OBSERVATION, NOT A VERDICT: the crate computes it from one poll with the
      // instant it was observed, because a relative age would freeze for the next 30 s.
      tray!.setToolTip(`Summrise — ${health}`);
      // THE MENU'S ROWS ARE DATA (`tray-menu.js`), so the wording, order, version row and the exact
      // action set are tested without Electron. This file owns only what each action DOES.
      const rows = require("./tray-menu").trayMenuRows({ version: trayFacts.version, health }) as Array<{
        label?: string; action?: string; type?: string; enabled?: boolean;
      }>;
      tray!.setContextMenu(Menu.buildFromTemplate(rows.map((row) => {
        if (row.type === "separator") return { type: "separator" as const };
        switch (row.action) {
          case "transcript": return { label: row.label, click: () => openDevicePage("activity") };
          case "panel": return { label: row.label, click: () => openDevicePage("panel") };
          case "browser": return { label: row.label, click: () => openHost("browser") };
          case "harness": return { label: row.label, click: () => openHost("harness") };
          case "fallback": return { label: row.label, click: () => openFallbackShell() };
          case "update": return {
            label: row.label,
            click: () => {
              void win?.focus();
              dialog.showMessageBox(win ?? undefined as unknown as BrowserWindow, {
                type: "info",
                title: "Summrise",
                message: trayFacts.version ? `Version ${trayFacts.version}` : "Version not reported",
                detail: "Updates are installed by the Summrise setup, not downloaded by the app. "
                  + "Running the setup again upgrades this install and keeps it serving.",
                buttons: ["OK"],
              });
            },
          };
          case "quit": return { label: row.label, click: () => { (app as any).isQuitting = true; app.quit(); } };
          default: return { label: row.label, enabled: false };
        }
      })));
    };
    refreshTray();
    setInterval(() => { refreshTray(); }, TRAY_POLL_MS);
  });
}

app.on("before-quit", () => {
  (app as any).isQuitting = true;
  // Close every browser-session window explicitly — app.quit() tears down the main window but
  // independent BrowserWindows with pending beforeunload handlers can stall or leak on exit. The
  // sweep itself lives with the table it walks (`browser-sessions.ts`).
  browserCloseAll();
});
