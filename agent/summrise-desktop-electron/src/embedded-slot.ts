// ── ONE EMBEDDED SLOT, INSTANTIATED TWICE ──────────────────────────────────────────────────────
// The shell embeds TWO native `WebContentsView`s over the SPA's placeholders: the real-render BROWSER
// view (an internet browser an AI also drives over CDP) and the DSH view (the operator's own harness on
// loopback). They arrived here as two modules — `embedded-view.ts` and `dsh-view.ts` — that a
// function-by-function comparison (`docs/agents/ideas.md`, round 111) measured to be near-copies of one
// pattern: the same eight twinned operations (`bind`/`deps`/`ensure`/`place`/`navigate`/`reload`/
// `recover`/`state`), the same three slot-state bindings, and ELEVEN occurrences of
// `setVisible`/`setBounds`/`addChildView`/`removeChildView` on each side. This file is that pattern,
// ONCE, as a factory: `embeddedSlot(config)` returns one slot's functions, and `main.ts` builds both
// instances from it.
//
// ── WHAT IS A PARAMETER, AND WHY EACH ONE CANNOT BE MERGED AWAY ────────────────────────────────
//   * `door` — the ONE load door for this slot: `loadTarget` (the url policy's `sanitizeBrowserUrl`,
//     admitting any http/https) for the browser, `dshTargetFor` (its own loopback origin, or nothing)
//     for the DSH. Forced by the crate split: the two policy crates must not depend on each other, so
//     shell-policy takes the ANSWER of url-policy's predicate. Merging them would either expose the
//     internet to the DSH's `window.open` or confine the browser view to loopback.
//   * `popupTarget` — the `target=_blank` decision, over the same split as `door`:
//     `embeddedPopupTarget(door(raw))` for the browser (its argument is the DECIDED target, and a
//     refusal is DROPPED rather than collapsed to `about:blank`, so a refused link cannot erase the page
//     being read) against `dshPopupTarget(raw, isDshUrl(raw))` for the DSH.
//   * `recoverUrl(lastDecided)` — what a crashed view is reloaded with: the browser's `embeddedRecoverUrl`
//     (the last DECIDED url, `about:blank` as the fallback, because "recovering a crashed view must not
//     make a request to a search engine to do it") against the DSH's `dshHome(base)` ("a crash recovery
//     that lands somewhere other than the harness is a view the operator did not ask for").
//   * `stateUrl(live, lastDecided)` — the `url` field of `state()`. The browser's address bar must never
//     go blank, so it answers the last decided url when its view cannot (`shownUrl`); the DSH's record
//     must not report a dead object's last url, so it answers "" (`shownUrl(live, "")`). This is a
//     FINDING rather than one of the four load-bearing differences above: the two modules differed here
//     by accident of their callers, and merging it either way would change one pane's contract.
//   * `goneChannel` — `embedded-browser:gone` / `embedded-dsh:gone`. The signal is the same on both
//     sides (a renderer that died must not leave the pane saying "starting…" forever); only the channel
//     name differs, and the names are pinned by the preload/panel fixtures.
//   * `navChannel?` — present ONLY for the browser: it brings the three navigation pushes
//     (`did-navigate`, `did-navigate-in-page`, `page-title-updated`), the `canBack`/`canFwd` fields of
//     `state()`, and the `go`/`zoom` operations. The DSH has no history surface, so adding them there
//     would be uncalled surface (and a channel the preload does not expose would fail the channel-set
//     equality in `test/ipc-door.test.mjs`).
//
// ── WHAT IS DELIBERATELY NOT A PARAMETER, AND WHY ──────────────────────────────────────────────
// THE LIFECYCLE AND THE STARTUP NAVIGATION ARE CALLER BEHAVIOURS. The browser slot is created EAGERLY at
// startup, hidden, and never navigated (an attached AI always has an `about:blank` CDP target; it used
// to load `bing.com` on every launch) — `main.ts` calls `ensure()` from `app.whenReady()`. The DSH slot
// is created LAZILY by the pane's `open()` and navigated home — `main.ts`'s `dshOpen()` is
// `slot.navigate(dshHome(dshBase()))`, and `navigate()` is what creates the view. Neither belongs in a
// parameter of `ensure()`: a flag here would let one caller ask for the other's lifecycle by accident.
// `main.ts` also keeps `bootDshPort`, `dshTargetFor`, `loadHarnessDoors`, `dshOpen` and EVERY
// `ipcHandle(...)` registration, because the shell holds the ONE `ipcMain.handle` door.
//
// A POLICY THAT IS THE SAME FOR BOTH SLOTS STAYS AN IMPORT, not a parameter: the minimum slot
// (`slotTooSmall`), the visible conjunction (`viewVisible`), the history sign (`goBackwards`) and the
// zoom clamp (`zoomFactor`) are the shell policy crate's answers and are identical for the two views.
// Only what DIFFERS travels as a parameter.
//
// ── FIVE THINGS THE TWO IMPLEMENTATIONS DISAGREED ABOUT THAT WERE NOT IN THE LIST ABOVE ────────
// Each was decided here explicitly rather than inherited from whichever side happened to be copied:
//   1. THE RE-SHOW AFTER A CRASH. Both `recover()`s contained a "re-show if the slot is live" block that
//      could never run: `ensure()` — which recovery has just called to build the fresh view — clears the
//      visible flag first. It is FIXED here rather than deleted: the block's own comment states its
//      purpose ("kept so a renderer-crash recovery can re-show the fresh view at the same spot without
//      waiting for the SPA"), the retained bounds exist for exactly that, and the fix makes the slot's
//      record agree with its screen (the flag is restored, so `state().visible` is true when the fresh
//      view is shown). It is not a new always-on behaviour: after a REAL crash the `gone` handler has
//      already cleared the flag, so the SPA's next `place()` still shows the view — this only covers a
//      recovery invoked without that event (an explicit recover, or a stalled renderer).
//   2. `ensure()` WAS EXPORTED BY ONE MODULE AND PRIVATE IN THE OTHER. The slot exposes it for both: it
//      is the browser's eager-creation hook, and the difference is which CALLER calls it, not what it
//      does. The DSH's instance is simply never asked.
//   3. THE PERMISSION HANDLERS. The browser's `ensure()` installed deny-by-default request/check
//      handlers on the session; the DSH's installed none. Both instances install them now: the DSH is
//      still a whole web application on loopback, and the DSH's own suite already called the posture it
//      lacked "the browser view's security posture". The calls are idempotent and inside the same
//      try/catch, so a session that refuses them is non-fatal on both sides.
//   4. THE `gone` HANDLER'S ORDER. The browser checked the window FIRST and returned — leaving a dead
//      renderer's view visible — where the DSH hid the view first and only the PUSH depended on the
//      window. The DSH's order is the rule here, on both slots: a renderer that died never repaints, the
//      hide needs nothing from the host, and the window (which `main.ts` re-creates and nulls) is read
//      only where it is genuinely required.
//   5. `state()`'s `visible` FIELD reached the crate on one side (`viewVisible`) and was spelled
//      `dshVisible && !!wc` on the other. The two are the same answer, so both go through the crate —
//      the decision belongs to the policy, not to one call site.
import { BrowserWindow, WebContentsView, session } from "electron";
import { goBackwards, slotTooSmall, viewVisible, zoomFactor } from "./summrise_shell_policy";

/** A slot's placement: CSS pixels in the window's content coordinate space. `null` is the SPA saying it
 *  has no slot (its page was left, or its placeholder unmounted), which hides the view. */
export type SlotBounds = { x: number; y: number; width: number; height: number };

/** The host side of one slot: what `main.ts` owns and lends. Every entry is a FUNCTION rather than a
 *  value, for the three reasons the old seams recorded: the window does not exist yet at module load and
 *  is nulled on close; the doors compose policy-crate predicates the host evaluates (the two crates must
 *  not depend on each other); and `setDshPort` runs at `whenReady`, so a snapshot of the DSH's base
 *  taken at load time would pin the address before the boot had read it. */
export type SlotConfig = {
  /** The shell's main window, or null while it does not exist. A GETTER, not a value. */
  win: () => BrowserWindow | null;
  /** The ONE load door for this slot: the raw url in, the DECIDED url out. */
  door: (raw: string) => string;
  /** The `target=_blank` decision over this slot's own door: the target to navigate to, or `undefined`
   *  to DROP the click (never `about:blank`, which would erase the page being read). */
  popupTarget: (raw: string) => string | undefined;
  /** What a crashed slot is reloaded with, given the last url the door decided for it. */
  recoverUrl: (lastDecided: string) => string;
  /** The `url` field of `state()`, given the live view's own answer (undefined when there is no live
   *  view) and the last decided url. */
  stateUrl: (live: string | undefined, lastDecided: string) => string;
  /** The channel a dead renderer's `gone` event is pushed on. */
  goneChannel: string;
  /** The channel navigation state is pushed on. ABSENT means this slot has no history surface: no
   *  pushes, no `canBack`/`canFwd` in `state()`, and no `go`/`zoom` on the returned object. */
  navChannel?: string;
};

/** What a slot's state record says without a history surface (the DSH): exactly these four keys. */
export type SlotState = { ok: true; url: string; title: string; visible: boolean };
/** ...and with one (the browser): the same record plus the two history fields. */
export type SlotNavState = SlotState & { canBack: boolean; canFwd: boolean };

/** One slot's operations. The names are per instance at the call site in `main.ts`; the SHAPE is what
 *  the two instances share. */
export type EmbeddedSlot = {
  /** Build (or return) this slot's view, hidden and attached when a window exists. Idempotent. */
  ensure: () => WebContentsView;
  place: (bounds: SlotBounds | null) => void;
  navigate: (raw: string) => { ok: boolean; url: string };
  reload: () => { ok: boolean };
  recover: () => { ok: boolean };
  state: () => SlotState;
};
/** The browser slot's surface: `state()` also answers history, and the two history operations exist. */
export type EmbeddedNavSlot = Omit<EmbeddedSlot, "state"> & {
  state: () => SlotNavState;
  go: (delta: -1 | 1) => { ok: boolean };
  zoom: (factor: number) => { ok: boolean; factor?: number };
};

/**
 * Build ONE embedded slot. Call it twice — the browser view and the DSH view — and the differences
 * between them are the config's, not the code's.
 *
 * A slot without its host is refused HERE, at construction, rather than guarded at every use: the two
 * modules this replaces bound their host once and threw a loud `…Bind() was never called` from every
 * operation until they did. The rule was right (a slot with no door or no window must fail loudly, never
 * attach a view to nothing and quietly show a blank pane) and the moment moves earlier: a config missing
 * a piece is an error the caller reads at startup, and no half-built slot can exist at all.
 */
export function embeddedSlot(config: SlotConfig & { navChannel: string }): EmbeddedNavSlot;
export function embeddedSlot(config: SlotConfig): EmbeddedSlot;
export function embeddedSlot(config: SlotConfig): EmbeddedSlot | EmbeddedNavSlot {
  if (!config || typeof config !== "object") {
    throw new Error(
      "embedded-slot: a slot is built from its host config (win, door, popupTarget, recoverUrl, stateUrl, " +
      "goneChannel, and navChannel for a slot with a history surface) — there is no such thing as a default one",
    );
  }
  const REQUIRED = ["win", "door", "popupTarget", "recoverUrl", "stateUrl"] as const;
  for (const name of REQUIRED) {
    if (typeof config[name] !== "function") {
      throw new Error(
        `embedded-slot: ${name} is not a function — a slot is built with its host (main.ts lends both ` +
        `instances the shell's window getter, one load door, and the policy answers only that slot may use)`,
      );
    }
  }
  if (typeof config.goneChannel !== "string" || !config.goneChannel) {
    throw new Error(
      "embedded-slot: goneChannel is missing — a crashed renderer pushes `gone` so the pane can show a " +
      "recovery state instead of 'starting…' forever, and a slot that cannot push it must not be built",
    );
  }
  if (config.navChannel !== undefined && (typeof config.navChannel !== "string" || !config.navChannel)) {
    throw new Error("embedded-slot: navChannel is present but is not a channel name — omit it for a slot with no history surface");
  }
  const { win, door, popupTarget, recoverUrl, stateUrl, goneChannel, navChannel } = config;

  // ── THE PER-INSTANCE STATE, AND IT IS THE INSTANCE'S ─────────────────────────────────────────
  // Four bindings, closed over rather than module-level, which is what makes two slots independent: the
  // two modules this replaces each held their state at module scope, so a second instance was not
  // merely unused, it was unrepresentable. (`embeddedUrl` is the fourth rather than the third: the
  // browser's module held view/visible/bounds AND the last url its door decided, which recovery and
  // `state()` both read. The DSH's recover ignores it and its `stateUrl` does too.)
  let view: WebContentsView | null = null;
  let visible = false;
  let bounds: Electron.Rectangle | null = null;
  let lastDecided = "about:blank";

  function ensure(): WebContentsView {
    if (view && !view.webContents.isDestroyed()) return view;
    // ── THE HOST IS READ BEFORE ANYTHING IS BUILT, AND BEFORE ANY STATE IS SET ─────────────────────
    // An UNBOUND slot is one whose window getter refuses (`deps().win()` on a host whose `bind()` never
    // ran), and a refusal must leave the slot exactly as it was: nothing built, nothing assigned, no
    // handler wired. The read used to sit AFTER `view = built` — the order BOTH modules had before the
    // unification — so the loud throw left a fully built view in the slot's state that no window ever
    // showed: its crash handler was live, a later `state()`/`reload()` answered about it, and a second
    // `ensure()` returned it without reading the window again. Nothing below needs the window to BUILD a
    // view; the read is only about WHERE to attach one, so it happens first and a refusal creates
    // nothing. This is a deliberate behaviour change on the UNBOUND path only: a bound host's getter
    // returns exactly what it returned before, and reads it exactly once.
    const w = win();
    const built = new WebContentsView({
      webPreferences: {
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: true,
        // NO `preload` KEY, on either slot: zero Node surface in a renderer. The browser view renders
        // arbitrary internet pages; the DSH is a whole web application on loopback. A `preload` added
        // here would be added to BOTH views at once, which is why it is written down.
      },
    });
    built.setVisible(false);
    visible = false;
    // ── `target=_blank`, ONE DECISION PER SLOT ─────────────────────────────────────────────────
    // Real sites open every external link in a new window (Baidu's, the harness's docs), and this is a
    // single-tab pane: the popup itself is always DENIED and the SAME view moves instead. WHICH targets
    // qualify is the `popupTarget` parameter's — through the slot's own door, so a refused scheme is
    // dropped rather than navigated to `about:blank`.
    built.webContents.setWindowOpenHandler(({ url }) => {
      const decided = popupTarget(url);
      if (decided !== undefined) navigateDecided(decided);
      return { action: "deny" };
    });
    if (navChannel) {
      // Event-driven (no polling): the SPA's address bar and back/fwd buttons track the ACTUAL page, so
      // an in-page (hash) navigation and a title-only update are changes it has to see too.
      const push = (): void => {
        const w = win();
        if (!w || w.isDestroyed()) return;
        const s = stateWithHistory();
        w.webContents.send(navChannel, { url: s.url, canBack: s.canBack, canFwd: s.canFwd, title: s.title });
      };
      built.webContents.on("did-navigate", push);
      built.webContents.on("did-navigate-in-page", push);
      built.webContents.on("page-title-updated", push);
    }
    // A renderer that crashed (process-gone) or hung mid-load never paints again. The dead view is HIDDEN
    // first — that needs nothing from the host — so the pane's recovery banner, which paints under the
    // native view, becomes visible in the slot; only the PUSH needs a live window, which `main.ts`
    // re-creates and nulls (a destroyed window's `send` throws in Electron).
    built.webContents.on("render-process-gone", (_e, details) => {
      try { built.setVisible(false); } catch { /* already gone */ }
      visible = false;
      const w = win();
      if (w && !w.isDestroyed()) {
        w.webContents.send(goneChannel, { reason: details.reason, exitCode: details.exitCode });
      }
    });
    try {
      session.defaultSession.setPermissionRequestHandler((_wc, _perm, cb) => cb(false));
      session.defaultSession.setPermissionCheckHandler(() => false);
    } catch { /* non-fatal (already set at app ready, or a session that does not exist) */ }
    view = built;
    // Attach to the main window's content view once the window exists — THE WINDOW READ AT THE TOP of
    // this function, not a second read: the same one getter call, moved ahead of the build. The view is
    // added HIDDEN and only shown when the SPA's page is active and reports its placeholder bounds.
    if (w) w.contentView.addChildView(built);
    return built;
  }

  /** Place the view over the SPA's slot (CSS px in the window's content coordinate space). Empty bounds
   *  hide it. */
  function place(next: SlotBounds | null): void {
    const v = ensure();
    const w = win();
    if (!w) return;
    if (slotTooSmall(next)) {
      v.setVisible(false);
      visible = false;
      return;
    }
    bounds = next;
    v.setBounds(next);
    v.setVisible(true);
    visible = true;
    // Keep it above the SPA content (the SPA's placeholder is an empty div).
    w.contentView.addChildView(v);
  }

  /** Load an ALREADY DECIDED url. The one path that does not open the door again: the popup decision and
   *  `recover()` both hand this a target their own policy has decided, and re-deciding it would be a
   *  second call the caller did not ask for. */
  function navigateDecided(url: string): void {
    const v = ensure();
    lastDecided = url;
    v.webContents.loadURL(url).catch(() => { /* did-fail-load surfaces in state() */ });
  }

  function navigate(raw: string): { ok: boolean; url: string } {
    // Every raw url the SPA or the AI hands this slot is decided by the ONE load door before it reaches
    // loadURL — and it is recorded FIRST, so the record is the door's answer and never the raw string.
    const url = door(raw);
    navigateDecided(url);
    return { ok: true, url };
  }

  function reload(): { ok: boolean } {
    const v = view;
    if (!v || v.webContents.isDestroyed()) return { ok: false };
    try { v.webContents.reload(); return { ok: true }; } catch { return { ok: false }; }
  }

  /** The record WITHOUT a history surface: exactly the four keys the DSH's pane reads. */
  function stateRecord(): SlotState {
    const v = view;
    const wc = v && !v.webContents.isDestroyed() ? v.webContents : null;
    return {
      ok: true,
      url: stateUrl(wc ? wc.getURL() : undefined, lastDecided),
      title: wc ? wc.getTitle() : "",
      visible: viewVisible(visible, !!wc),
    };
  }

  /** ...and the same record WITH the two history fields, which only the browser's slot has. */
  function stateWithHistory(): SlotNavState {
    const v = view;
    const wc = v && !v.webContents.isDestroyed() ? v.webContents : null;
    return {
      ...stateRecord(),
      canBack: !!wc && wc.navigationHistory.canGoBack(),
      canFwd: !!wc && wc.navigationHistory.canGoForward(),
    };
  }

  /** Recover after a renderer crash: a dead webContents never recovers, so re-create the view. */
  function recover(): { ok: boolean } {
    // THE SLOT'S PLACEMENT IS READ BEFORE THE RE-CREATE. `ensure()` clears the visible flag for the view
    // it builds, so a re-show block that reads the flag afterwards can never run — which is exactly the
    // defect both modules carried. Reading it here is what makes the re-show below reachable, and the
    // bounds are retained on every hide for the same reason (they are the SPA's last placement).
    const wasVisible = visible;
    const lastBounds = bounds;
    try {
      if (view && !view.webContents.isDestroyed()) {
        view.webContents.close({ waitForBeforeUnload: false });
      }
      if (view) {
        try { win()?.contentView.removeChildView(view); } catch { /* already gone */ }
        view = null;
      }
    } catch { /* fall through to ensure() */ }
    const fresh = ensure();
    if (!fresh || fresh.webContents.isDestroyed()) return { ok: false };
    // The recovery target is the parameter's, and it is ALREADY A DECISION: the browser's re-loads the
    // last url its door decided (never a third-party home page a crash would fetch), the DSH's is its
    // own home (`dshHome`), because a recovery that lands anywhere else is a view nobody asked for.
    fresh.webContents.loadURL(recoverUrl(lastDecided)).catch(() => { /* did-fail-load surfaces */ });
    // Re-show at the same spot if the slot was live when the renderer died — and restore the flag, so the
    // record the pane reads agrees with what is on screen. A slot that was hidden (never placed, or the
    // SPA cleared its placeholder) stays hidden: the SPA's next place() is what shows it.
    if (wasVisible && lastBounds) {
      const w = win();
      if (w) {
        fresh.setBounds(lastBounds);
        fresh.setVisible(true);
        visible = true;
        w.contentView.addChildView(fresh);
      }
    }
    return { ok: true };
  }

  const slot: EmbeddedSlot = { ensure, place, navigate, reload, recover, state: stateRecord };
  if (!navChannel) return slot;
  // THE HISTORY SURFACE, only for the slot that declared a nav channel. A pane with no address bar and no
  // back/fwd buttons has no use for these, and a channel nothing invokes is a channel the preload does not
  // expose — which `test/ipc-door.test.mjs` fails on.
  const nav: EmbeddedNavSlot = {
    ...slot,
    state: stateWithHistory,
    /** Back/forward through the REAL webContents history; the direction sign is the crate's. */
    go: (delta: -1 | 1): { ok: boolean } => {
      const v = view;
      if (!v || v.webContents.isDestroyed()) return { ok: false };
      try {
        if (goBackwards(delta)) v.webContents.goBack();
        else v.webContents.goForward();
        return { ok: true };
      } catch { return { ok: false }; }
    },
    /** Zoom the REAL view (webContents.setZoomFactor — the native browser zooms, not a CSS scale). The
     *  clamp AND the falsy test are the crate's, because `zoomFactor(0)` is 100% and not 50%. */
    zoom: (factor: number): { ok: boolean; factor?: number } => {
      const v = view;
      if (!v || v.webContents.isDestroyed()) return { ok: false };
      const f = zoomFactor(factor);
      try { v.webContents.setZoomFactor(f); return { ok: true, factor: f }; } catch { return { ok: false }; }
    },
  };
  return nav;
}
