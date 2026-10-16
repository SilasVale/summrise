// ── The DSH view — the harness's own UI, embedded in the shell ──────────────────────────────
// The DSH (the operator's own agent harness) runs on THIS machine, and the panel's DSH page
// shows it in a native `WebContentsView` over the SPA's slot. This file is that view: its
// state (the view, its visibility, its last bounds) and the seven operations the shell and the
// SPA drive it with.
//
// ── WHY THIS IS A FILE OF ITS OWN, AND THE FOUR THINGS IT DOES NOT OWN ──────────────────────
// This is `main.ts`'s DSH section, moved out whole and VERBATIM. The reason is not tidiness: a
// function-by-function comparison found this cluster to be a near-copy of `embedded-view.ts`,
// and the two are to be UNIFIED into one parametrised slot. That unification cannot happen
// before this cluster has tests, and it cannot have tests while it lives in `main.ts` — no test
// can import that file, because importing it runs the whole application. Moving it here is what
// makes today's tests (and tomorrow's unification) possible.
//
// It has FOUR dependencies outside itself, and they are why this file takes a SEAM rather than
// an import. Importing any of them from `main.ts` would make `main.ts` require this module and
// this module require `main.ts`: a cycle whose failure mode is a half-initialised module rather
// than an error. So `main.ts` binds all four ONCE (`dshViewBind`) and this module reads them
// through `deps()`.
//   * the main window (`win`) — every view here is attached to the shell's window, which
//     `main.ts` creates, re-creates and nulls. A copy taken at load time would go stale, so it
//     is a GETTER.
//   * the ONE load door (`targetFor`) — `main.ts`'s `dshTargetFor`, which is the url policy's
//     `isDshUrl` admission composed with the shell policy's serialization. Every URL this view
//     navigates to goes through it, never around it, and the door's DECISION (its own origin,
//     or `about:blank`) is the policy's rather than a comparison written here.
//   * the url policy's ADMISSION (`admitted`) — `isDshUrl`, and it is bound rather than
//     imported because it is the URL POLICY's predicate and the two policy crates must not each
//     hold a copy of its port state (the import block in `main.ts` measures why). The crate's
//     `dsh_target` and `dsh_popup_target` both document it this way: "`admitted` is
//     `isDshUrl(raw)`, evaluated by the host".
//   * the DSH's BASE (`base`) — `dshBase()`, the address the harness answers on. `dshHome`'s own
//     doc says why it arrives from here: "the BASE arrives from the host, which is the only
//     thing that can read the configured DSH port" — `setDshPort` is `main.ts`'s call.
// This module therefore imports NEITHER url-policy name, and holds no copy of the port list.
//
// ── WHAT DELIBERATELY DID NOT MOVE, AND WHY ─────────────────────────────────────────────────
// THE IPC REGISTRATIONS. They go through `main.ts`'s ONE door (`ipcHandle`, which applies the
// frame check and is the shell's only `ipcMain.handle` call), and the channel set is pinned
// against the preload's by `test/ipc-door.test.mjs`. A handler registered here would be a
// second door, so `main.ts` keeps the `ipcHandle("embedded-dsh:…")` lines and calls in.
//
// THE THREE HOST FUNCTIONS, one paragraph each, because each looks like it belongs here:
//   * `bootDshPort()` is the BOOT's port pin, the twin of `bootAgentPort()` that stayed beside
//     it: it is called once, from `app.whenReady()`, as the argument to `setDshPort` — it FEEDS
//     the url policy's port state that `main.ts` owns. No operation of this view calls it.
//   * `dshTargetFor()` IS the one load door above. It stays because `main.ts` owns the load door
//     of every view it lends one to (`loadTarget` for the other two modules), and because it is
//     where that file evaluates the url-policy predicate `isDshUrl` — the two-crate rule its
//     import block states. Moving it here would put a url-policy call in a second file.
//   * `loadHarnessDoors()` is not this view's operation at all: it is the boot's ONE read of the
//     agent's harness table, which MUTATES the url policy's own port list (`addDshPort`),
//     fetches with the device token (`authHeaders`) and is bounded by the boot table's
//     `HARNESS_FETCH_MS`. The view never calls it; lending this module a credential and a boot
//     number for a caller it does not have is dependency surface bought for nothing.
import { BrowserWindow, WebContentsView } from "electron";
import { dshHome, dshPopupTarget, slotTooSmall } from "./summrise_shell_policy";

/** The host side of this module's seam: what `main.ts` owns and lends to the DSH view. */
export type DshViewHost = {
  /** The shell's main window, or null while it does not exist. A GETTER, not a value: `main.ts`
   *  re-creates the window (and nulls it on close), so a snapshot would outlive its window. */
  win: () => BrowserWindow | null;
  /** `main.ts`'s `dshTargetFor` — the ONE load door for this view. */
  targetFor: (raw: string) => string;
  /** The url policy's `isDshUrl` — whether the door admits this address at all. */
  admitted: (raw: string) => boolean;
  /** The url policy's `dshBase()` — the DSH's own base address. */
  base: () => string;
};
let bound: DshViewHost | null = null;
/** Called ONCE by `main.ts`, before any `embedded-dsh:*` channel can be invoked. */
export function dshViewBind(host: DshViewHost): void { bound = host; }
function deps(): DshViewHost {
  // LOUD, not silent: an unbound host would otherwise attach the view to nothing and navigate
  // through a fallback door — the failures a reader would have to reconstruct from a blank pane.
  if (!bound) throw new Error("dsh-view: dshViewBind() was never called — main.ts binds the shell's window, the DSH load door and the url policy's answers at startup");
  return bound;
}

let dshView: WebContentsView | null = null;
let dshVisible = false;
let dshBounds: Electron.Rectangle | null = null;
function dshViewEnsure(): WebContentsView {
  if (dshView && !dshView.webContents.isDestroyed()) return dshView;
  const view = new WebContentsView({
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
    const decided = dshPopupTarget(url, deps().admitted(url));
    if (decided !== undefined) dshNavigate(decided);
    return { action: "deny" };
  });
  view.webContents.on("render-process-gone", (_e, details) => {
    try { view.setVisible(false); } catch { /* already gone */ }
    dshVisible = false;
    // The SAME signal the browser view sends, for the same reason (round-256 there): without it
    // the pane shows "starting…" forever over a view that will never paint, and the operator has
    // no way to tell a slow load from a dead renderer.
    const w = deps().win();
    if (w && !w.isDestroyed()) {
      w.webContents.send("embedded-dsh:gone", { reason: details.reason, exitCode: details.exitCode });
    }
  });
  dshView = view;
  const w = deps().win();
  if (w) w.contentView.addChildView(view);
  return view;
}
export function dshNavigate(url: string): { ok: boolean; url: string } {
  const target = deps().targetFor(url);
  const view = dshViewEnsure();
  view.webContents.loadURL(target).catch(() => { /* did-fail-load surfaces in state() */ });
  return { ok: true, url: target };
}
/** Open (or re-focus) the DSH at its configured address. Idempotent.
 *
 *  `dshHome` is the policy crate's and it was spelled TWICE in this file before the port — here and in
 *  `dshRecover` — which is exactly the pair that must not drift: a crash recovery that lands somewhere
 *  other than the harness is a view the operator did not ask for. */
export function dshOpen(): { ok: boolean; url: string } {
  return dshNavigate(dshHome(deps().base()));
}
/** Place the view over the SPA's slot. Empty bounds hide it (the same contract as the browser). */
export function dshPlace(bounds: { x: number; y: number; width: number; height: number } | null): void {
  const view = dshViewEnsure();
  const w = deps().win();
  if (!w) return;
  // The SAME predicate the browser view uses — one `MIN_SLOT_PX`, two call sites, before the port.
  if (slotTooSmall(bounds)) {
    view.setVisible(false);
    dshVisible = false;
    return;
  }
  dshBounds = bounds;
  view.setBounds(bounds);
  view.setVisible(true);
  dshVisible = true;
  w.contentView.addChildView(view);
}
export function dshReload(): { ok: boolean } {
  const view = dshView;
  if (!view || view.webContents.isDestroyed()) return { ok: false };
  try { view.webContents.reload(); return { ok: true }; } catch { return { ok: false }; }
}
/** Recover after a renderer crash: a dead webContents never recovers, so re-create the view. */
export function dshRecover(): { ok: boolean } {
  try {
    if (dshView && !dshView.webContents.isDestroyed()) dshView.webContents.close({ waitForBeforeUnload: false });
    if (dshView) {
      try { deps().win()?.contentView.removeChildView(dshView); } catch { /* already gone */ }
      dshView = null;
    }
  } catch { /* fall through to ensure() */ }
  const view = dshViewEnsure();
  if (!view || view.webContents.isDestroyed()) return { ok: false };
  view.webContents.loadURL(dshHome(deps().base())).catch(() => { /* did-fail-load surfaces */ });
  const w = deps().win();
  if (dshVisible && w && dshBounds) {
    view.setBounds(dshBounds);
    view.setVisible(true);
    w.contentView.addChildView(view);
  }
  return { ok: true };
}
export function dshState(): { ok: true; url: string; title: string; visible: boolean } {
  const view = dshView;
  const wc = view && !view.webContents.isDestroyed() ? view.webContents : null;
  return { ok: true, url: wc ? wc.getURL() : "", title: wc ? wc.getTitle() : "", visible: dshVisible && !!wc };
}
