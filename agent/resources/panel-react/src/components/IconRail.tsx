// IconRail — shared by both densities: brand mark on top, the page icons,
// connection dot pinned to the foot. Uses the unified ui/Icon set.
// The page list comes from `PAGE_ICONS` below, which is a Record over `Page` —
// so adding a page is a compile error until it has an icon here.
//
// The foot dot reports the DEVICE state, not just connectivity: offline (no
// agent), idle (connected, nothing happening), working (activity within the
// last few seconds — see useDeviceActivity). Both densities render the same
// three states, because "is this machine busy" is a property of the machine and
// not of which shell is showing it.
import { useState } from "react";
import { Icon, BrandMark, type IconName } from "../ui/Icon";
import { getTheme, toggleTheme } from "../lib/theme";
import { useDeviceActivity } from "../hooks/useDeviceActivity";
import { deviceLiveness } from "../lib/liveness";
import type { Page } from "./Shell";

/** The page→icon contract for the rail, exported so the desktop header can draw
 *  the SAME glyph for a page as the rail button that opened it (that header used
 *  to carry a hand-written ternary chain, which is how a new page ends up with
 *  no icon there). */
export const PAGE_ICONS: Record<Page, IconName> = {
  terminal: "terminal",
  // RECORDED HISTORY (sessions + runs), not a live console: a stacked box with a lid. The
  // `activity` glyph went with the `activity` PAGE in round 31's merge — an icon nothing can draw
  // is not a spare part, it is a thing to keep in sync by hand.
  history: "archive",
  browser: "browser",
  memory: "memory",
  plugins: "plugins",
  settings: "settings",
};

export function IconRail({
  page,
  onPageChange,
  connected,
  desktop,
  pendingCount = 0,
  commandsInFlight,
  onOpenGuide,
}: {
  page: Page;
  onPageChange: (p: Page) => void;
  /**
   * IS THE SSE STREAM UP — not "do we hold credentials", which is what `App` means by the same word
   * when it passes `connected` to `useSessions`/`usePlugins`. Two facts, one name, and the panel
   * exploration flagged it (2026-09-24).
   *
   * RENAMING IT IS A MEASURED JOB, NOT A GREP: a dry run over `connected` as an identifier found 31
   * sites in six files, and `DesktopShell` was the trap — 9 of its 11 are `vitals.relay.connected` and
   * the labels around it, which is the RELAY's fact and must not move. Six occurrences are quoted
   * (`"connected"` as the sseState value, `"disconnected"` as a label) and stay. When this is renamed,
   * use exact per-site pairs rather than a word-boundary pass, and let `tsc` prove it.
   */
  connected: boolean;
  desktop?: boolean;
  /** Sessions holding a question for the operator (see `pendingApprovalCount`).
   *  Optional and defaulted so a caller that has no session list (or an older
   *  embedding) cannot crash the rail — it degrades to "no questions waiting". */
  pendingCount?: number;
  /** At least one session is holding a command in flight — the device's `command_running`, at device scope. */
  commandsInFlight?: boolean;
  /** Reopen the getting-started guide. Absent on a surface that does not host it. */
  onOpenGuide?: () => void;
}) {
  const btn = (active: boolean) =>
    desktop
      ? `desktop-rail-btn${active ? " active" : ""}`
      : `rail-btn${active ? " active" : ""}`;
  const [theme, setThemeState] = useState(getTheme());
  const themeBtnClass = desktop ? "desktop-rail-btn" : "rail-btn";
  const flipTheme = () => setThemeState(toggleTheme());
  // TWO SIGNALS, ONE MARK. `useDeviceActivity` is recency — an activity frame within the last window — and it is
  // right about "something just happened". It cannot see a command that is running QUIETLY, which is the whole
  // point of the flag the device now reports; the session list that carries it is the caller's, passed in like
  // `pendingCount` rather than re-derived here.
  const sseWorking = useDeviceActivity();
  const working = sseWorking || !!commandsInFlight;
  // WAITING OUTRANKS WORKING. Both can be true at once (the AI asked, then kept
  // working elsewhere), and of the two, "a decision is waiting for you" is the
  // one that decays if it goes unnoticed: the question expires. Precedence, in
  // order: no transport → off (nothing can be answered anyway), a question →
  // waiting, activity → working, else idle.
  const waiting = pendingCount > 0;
  // ONE MODEL, ONE PLACE. This was an inline ternary, which made it the panel's entire state model — and a
  // second copy of it in another component is how two surfaces come to disagree about the same device.
  const state = deviceLiveness({ connected, pendingCount, working });
  const label = !connected
    ? "disconnected"
    : waiting
      ? `${pendingCount} command${pendingCount === 1 ? "" : "s"} waiting for your answer`
      : working
        ? "device is working"
        : "device is idle";
  return (
    <>
      {desktop ? (
        <div className="desktop-rail-brand" title="Summrise">
          <BrandMark size={26} />
        </div>
      ) : (
        <div className="rail-brand" title="Summrise">
          <BrandMark size={20} />
        </div>
      )}
      {(Object.keys(PAGE_ICONS) as Page[]).map((p) => (
        <button
          key={p}
          type="button"
          className={btn(page === p)}
          aria-current={page === p ? "page" : undefined}
          title={p[0].toUpperCase() + p.slice(1)}
          aria-label={p[0].toUpperCase() + p.slice(1)}
          onClick={() => onPageChange(p)}
        >
          <Icon name={PAGE_ICONS[p]} size={desktop ? 18 : 20} />
        </button>
      ))}
      {/* theme toggle — light is the default; dark is the optional cockpit */}
      <button
        type="button"
        className={themeBtnClass}
        title={theme === "dark" ? "Switch to light" : "Switch to dark"}
        aria-label="theme"
        onClick={flipTheme}
      >
        <Icon
          name={theme === "dark" ? "sun" : "moon"}
          size={desktop ? 16 : 18}
        />
      </button>
      {/* THE GUIDE'S WAY BACK. A first-run card that can only be seen once is a card somebody
          closes by accident and then resents; this sits next to the other rail controls, where
          a lost reader looks. */}
      {onOpenGuide && (
        <button
          type="button"
          className={themeBtnClass}
          title="Getting started"
          aria-label="Getting started"
          onClick={onOpenGuide}
        >
          <Icon name="help" size={desktop ? 16 : 18} />
        </button>
      )}
      {desktop ? (
        <>
          {/* The shared `.mark` block draws the silhouette; the rail only places it. */}
          <div className="desktop-rail-status" title={label}>
            <span className="mark dot" data-live={state} />
          </div>
        </>
      ) : (
        <div className="rail-spacer" />
      )}
      {!desktop && (
        <div className="mark rail-dot" data-live={state} title={label} />
      )}
    </>
  );
}
