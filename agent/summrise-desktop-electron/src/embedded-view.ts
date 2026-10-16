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
//
// ── WHY THIS IS A FILE OF ITS OWN, AND THE TWO THINGS IT DOES NOT OWN ───────────────────────
// This is `main.ts`'s embedded-browser section, moved out whole. The survey that found it (the
// desktop-shell round) measured that nothing in it touches `tray`, `Tray`, `Menu`, `mainWindow` or
// `ipcMain` — TRUE, and not the whole story: it has TWO dependencies outside that list, and they are
// why this file takes a seam rather than an import.
//   * the main window (`win`) — every view here is attached to the shell's window, which `main.ts`
//     creates, re-creates and nulls. A copy taken at load time would go stale, so it is a GETTER.
//   * the ONE load door (`loadTarget`) — `main.ts`'s single call site of the URL policy's
//     `sanitizeBrowserUrl`. Every URL this view loads must go through it, never around it.
// Importing either from `main.ts` would make `main.ts` require this module and this module require
// `main.ts`: a cycle whose failure mode is a half-initialised module rather than an error. So
// `main.ts` binds both ONCE (`embeddedViewBind`) and this module reads them through `deps()`.
//
// THE IPC REGISTRATIONS DELIBERATELY DID NOT MOVE WITH IT. They go through `main.ts`'s ONE door
// (`ipcHandle`, which applies the frame check and is the file's only `ipcMain.handle` call), and the
// channel set is pinned against the preload's by `test/ipc-door.test.mjs`. A handler registered here
// would be a second door, so `main.ts` keeps the `ipcHandle("embedded-browser:…")` lines and calls in.
import { BrowserWindow, WebContentsView, session } from "electron";
import {
  embeddedPopupTarget, embeddedRecoverUrl, goBackwards, shownUrl, slotTooSmall, viewVisible,
  zoomFactor,
} from "./summrise_shell_policy";

/** The host side of this module's seam: what `main.ts` owns and lends to the embedded view. */
export type EmbeddedViewHost = {
  /** The shell's main window, or null while it does not exist. A GETTER, not a value: `main.ts`
   *  re-creates the window (and nulls it on close), so a snapshot would outlive its window. */
  win: () => BrowserWindow | null;
  /** `main.ts`'s `loadTarget` — the ONE load door (the URL policy's `sanitizeBrowserUrl`). */
  loadTarget: (raw: string | undefined) => string;
};
let bound: EmbeddedViewHost | null = null;
/** Called ONCE by `main.ts`, before any `embedded-browser:*` channel can be invoked. */
export function embeddedViewBind(host: EmbeddedViewHost): void { bound = host; }
function deps(): EmbeddedViewHost {
  // LOUD, not silent: an unbound host would otherwise attach the view to nothing and load URLs
  // through a fallback door — the failures a reader would have to reconstruct from a blank page.
  if (!bound) throw new Error("embedded-view: embeddedViewBind() was never called — main.ts binds the shell's window and its load door at startup");
  return bound;
}

let embeddedView: WebContentsView | null = null;
let embeddedVisible = false;
let embeddedUrl = "about:blank";
// round-256: last placed bounds — kept so a renderer-crash recovery can
// re-show the fresh view at the same spot without waiting for the SPA.
let embeddedBounds: Electron.Rectangle | null = null;
export function embeddedViewEnsure(): WebContentsView {
  if (embeddedView && !embeddedView.webContents.isDestroyed()) return embeddedView;
  const view = new WebContentsView({
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
    const decided = embeddedPopupTarget(deps().loadTarget(url));
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
    session.defaultSession.setPermissionRequestHandler((_wc, _perm, cb) => cb(false));
    session.defaultSession.setPermissionCheckHandler(() => false);
  } catch { /* non-fatal (already set at app ready) */ }
  embeddedView = view;
  // Attach to the main window's content view once the window exists. The
  // view is added hidden and only shown when the SPA's Browser page is
  // active and reports its placeholder bounds.
  const w = deps().win();
  if (w) w.contentView.addChildView(view);
  return view;
}
/** Place the embedded view over the SPA's browser placeholder (CSS px in the
 *  window's content coordinate space). Empty bounds hide it. */
export function embeddedViewPlace(bounds: { x: number; y: number; width: number; height: number } | null): void {
  const view = embeddedViewEnsure();
  const w = deps().win();
  if (!w) return;
  // `K.MIN_SLOT_PX` AND ITS COMPARISON ARE ONE FUNCTION NOW: this view and the DSH view each spelled
  // `bounds.width < 50 || bounds.height < 50` BEFORE the port, which is two copies of one policy — the
  // place where the browser view and the DSH view would come to disagree about what "showing" means.
  if (slotTooSmall(bounds)) {
    view.setVisible(false);
    embeddedVisible = false;
    return;
  }
  embeddedBounds = bounds;
  view.setBounds(bounds);
  view.setVisible(true);
  embeddedVisible = true;
  // Keep it above the SPA content (the SPA placeholder is an empty div).
  w.contentView.addChildView(view);
}
export function embeddedNavigate(raw: string): { ok: boolean; url: string; error?: string } {
  // Every raw URL the SPA/AI hands the embedded view is decided by the ONE
  // load door before it reaches loadURL.
  const url = deps().loadTarget(raw);
  const view = embeddedViewEnsure();
  embeddedUrl = url;
  view.webContents.loadURL(url).catch(() => { /* did-fail-load surfaces */ });
  return { ok: true, url };
}
/** round-247: nav controls on the embedded view (the SPA's toolbar mirrors a
 *  real browser: back/fwd/reload operate the actual webContents history). */
export function embeddedGo(delta: -1 | 1): { ok: boolean } {
  const view = embeddedView;
  if (!view || view.webContents.isDestroyed()) return { ok: false };
  try {
    if (goBackwards(delta)) view.webContents.goBack();
    else view.webContents.goForward();
    return { ok: true };
  } catch { return { ok: false }; }
}
export function embeddedReload(): { ok: boolean } {
  const view = embeddedView;
  if (!view || view.webContents.isDestroyed()) return { ok: false };
  try { view.webContents.reload(); return { ok: true }; } catch { return { ok: false }; }
}
export function embeddedState(): { ok: true; url: string; canBack: boolean; canFwd: boolean; title: string; visible: boolean } {
  const view = embeddedView;
  const wc = view && !view.webContents.isDestroyed() ? view.webContents : null;
  return {
    ok: true,
    // The FALLBACK is the policy crate's: a live `getURL()` that answers "" (not yet navigated) and no
    // live contents at all are the same answer, and it is the last URL this view was sent to.
    url: shownUrl(wc ? wc.getURL() : undefined, embeddedUrl),
    canBack: !!wc && wc.navigationHistory.canGoBack(),
    canFwd: !!wc && wc.navigationHistory.canGoForward(),
    title: wc ? wc.getTitle() : "",
    visible: viewVisible(embeddedVisible, !!wc),
  };
}
/** round-247: push real navigation state (URL + title + history) to the SPA
 *  so its address bar and back/fwd buttons track the ACTUAL embedded page —
 *  event-driven (fires on navigation, no polling). */
export function embeddedWireNavEvents(view: WebContentsView): void {
  const wc = view.webContents;
  const push = (): void => {
    const w = deps().win();
    if (!w || w.isDestroyed()) return;
    const s = embeddedState();
    w.webContents.send("embedded-browser:nav", { url: s.url, canBack: s.canBack, canFwd: s.canFwd, title: s.title });
  };
  wc.on("did-navigate", push);
  wc.on("did-navigate-in-page", push);
  wc.on("page-title-updated", push);
  // round-256: the embedded view's RENDERER can crash (process-gone) or hang
  // mid-load (did-fail-load). Push a `gone` event so the SPA shows a recovery
  // state instead of "Starting…" forever; the SPA then offers a reload.
  wc.on("render-process-gone", (_e, details) => {
    const w = deps().win();
    if (!w || w.isDestroyed()) return;
    // Hide the dead view so the SPA's recovery banner (which paints under
    // the native view) becomes visible in the slot.
    try { view.setVisible(false); } catch { /* already gone */ }
    embeddedVisible = false;
    w.webContents.send("embedded-browser:gone", {
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
export function embeddedRecover(): { ok: boolean } {
  try {
    if (embeddedView && !embeddedView.webContents.isDestroyed()) {
      embeddedView.webContents.close({ waitForBeforeUnload: false });
    }
    if (embeddedView) {
      try { deps().win()?.contentView.removeChildView(embeddedView); } catch { /* already gone */ }
      embeddedView = null;
    }
  } catch { /* fall through to ensure() */ }
  const view = embeddedViewEnsure();
  if (!view || view.webContents.isDestroyed()) return { ok: false };
  // Already decided by loadTarget() when embeddedUrl was set (the ONE load
  // door) — recovery re-loads that decision, it does not re-open the policy.
  // The fallback is about:blank, NOT a third-party home page: recovering a
  // crashed view must not make a request to a search engine to do it. That
  // rule is `embeddedRecoverUrl`'s, in the policy crate.
  const url = embeddedRecoverUrl(embeddedUrl);
  view.webContents.loadURL(url).catch(() => { /* did-fail-load surfaces */ });
  // Re-show if the SPA slot is live (bounds were placed before the crash).
  const w = deps().win();
  if (embeddedVisible && w && embeddedBounds) {
    view.setBounds(embeddedBounds);
    view.setVisible(true);
    w.contentView.addChildView(view);
  }
  return { ok: true };
}
/** round-251: zoom the REAL embedded view (webContents.setZoomFactor — the
 *  native browser zooms, not a CSS scale). factor: 0.5-3.0. */
export function embeddedZoom(factor: number): { ok: boolean; factor?: number } {
  const view = embeddedView;
  if (!view || view.webContents.isDestroyed()) return { ok: false };
  // The clamp AND the `Number(factor) || 1` falsy test are the policy crate's, because `zoomFactor(0)`
  // is 100% and not 50% — a boundary that a hand-written `Math.min/Math.max` here got right by
  // accident and a rewrite would not.
  const f = zoomFactor(factor);
  try { view.webContents.setZoomFactor(f); return { ok: true, factor: f }; }
  catch { return { ok: false }; }
}
