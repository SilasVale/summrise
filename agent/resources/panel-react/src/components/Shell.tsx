// Shell — ONE shell for both densities (per the core design doc §4).
//   density="panel"   → icon rail | context rail | canvas + status bar
//   density="desktop" → icon rail | canvas, with the SAME full-width status bar under both (round 265).
//   It used to hide the status bar in this density, and DesktopShell drew its own inside the content
//   card — which is why the desktop strip started at the rail's edge while the panel's spanned the window.
// The density difference is PURELY visibility; navigation and pages are shared.
import type { ReactNode } from "react";

type Density = "panel" | "desktop";
/** The pages, in rail order. `archive` sits directly under `terminal` because it
 *  is the terminal page's own history: the device's RECORDED sessions, openable
 *  after the session — or the agent — is gone. `activity` follows, because it is
 *  the device-level answer to the question the terminal page answers
 *  per-session, and because it is the one page that works with no session at
 *  all, which is where an operator landing on an idle device starts. */
export type Page = "terminal" | "history" | "browser" | "harness" | "workspaces" | "memory" | "plugins" | "settings";

export const PAGES: Page[] = ["terminal", "history", "browser", "harness", "workspaces", "memory", "plugins", "settings"];
export const PAGE_LABELS: Record<Page, string> = {
  terminal: "Terminal",
  // One page for what the device recorded: sessions AND runs (round 31's merge).
  history: "History",
  browser: "Browser",
  // The AI client itself, embedded by the desktop shell. THE ID IS `harness` AND NOT `dsh`
  // because the rail DERIVES its label from the id (`p[0].toUpperCase() + p.slice(1)`) — an id of
  // `dsh` printed "Dsh" on the rail while this map said "Harness" in the header, which is two
  // surfaces disagreeing about one page. Measured on the device, not noticed by reading.
  harness: "Harness",
  // A path on a host, and the connection that reaches it. The rail derives its label from the id, so the
  // id is the word a person reads — the `dsh`/`harness` drift is why that is written down here.
  workspaces: "Workspaces",
  memory: "Memory",
  plugins: "Plugins",
  settings: "Settings",
};

export function Shell({ density, iconRail, contextRail, canvas, statusBar }: {
  density: Density;
  iconRail: ReactNode;
  contextRail?: ReactNode;   // panel density only
  canvas: ReactNode;
  /** BOTH densities (round 265). It was "panel density only" — and the desktop density paid for that in
   *  GEOMETRY: with no slot to render into, `DesktopShell` folded its strip into the content card, so the
   *  same device-level facts (version, uptime, CPU/MEM, relay, watches) were a full-width bar in one density
   *  and a card footer starting after the rail in the other. They are the same bar now, in the same place,
   *  and the operator's question — "why does the bottom bar not start at the window's edge?" — has the same
   *  answer in both. */
  statusBar?: ReactNode;
}) {
  if (density === "desktop") {
    return (
      <div className="desktop-shell">
        {/* THE RAIL AND THE CANVAS SHARE THE ROW, the bar gets the full width under both — exactly the
            shape the panel density has had all along (`#shell-main` + `{statusBar}`). */}
        <div className="desktop-body">
          <nav className="desktop-rail" aria-label="Pages">{iconRail}</nav>
          <main className="desktop-main">{canvas}</main>
        </div>
        {statusBar}
      </div>
    );
  }
  // round-161: the status bar is a BOTTOM BAR — it used to be the last flex
  // child of the row-direction #app-shell and rendered as a stray column on
  // the right edge.
  return (
    <div id="app-shell">
      <div id="shell-main">
        {/* LANDMARKS, like the desktop density already had: the panel's shell was four plain divs, so
            a screen reader had no way to jump between the rail, the page and the side list — and no
            way to tell which of the three regions it was in. */}
        <nav id="icon-rail" aria-label="Pages">{iconRail}</nav>
        {contextRail && <aside id="context-rail">{contextRail}</aside>}
        <main id="canvas-host">{canvas}</main>
      </div>
      {statusBar}
    </div>
  );
}
