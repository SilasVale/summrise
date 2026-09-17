// react-jsx: no React import needed
import { useState } from "react";
import type { Session } from "../hooks/useSessions";
import { useActiveTabVisible } from "../hooks/useActiveTabVisible";
import { useStripOverflow } from "../hooks/useStripOverflow";
import { disambiguateLabels } from "../lib/sessionLabels";
import { livenessOf } from "../lib/liveness";
import { Icon } from "../ui/Icon";

/** Per-session main-area view (round-admin-ui Task 5): the terminal pane +
 *  command card stream, the raw trajectory timeline, or the PATH — this
 *  session's work as a scannable list of steps plus a summary (design §2.1/§7). */
export type SessionView = "terminal" | "trajectory" | "path";

/**
 * HOW MANY THE STRIP IS HIDING — one component, because there are two strips.
 *
 * The panel's strip got this in round 168 and the desktop's did not; round 170 measured a rendered desktop
 * page with no count at all and an overflow flag that had silently become always-true when the hook's return
 * shape changed under it. Two renderers of one measurement is the drift this file's neighbours keep warning
 * about, so the chip is written once.
 *
 * aria-hidden because the count is a visual affordance for the STRIP: a screen reader reaching the tab list
 * already gets every tab, including the hidden ones.
 */
export function StripMore({ n }: { n: number }) {
  if (n <= 0) return null;
  return (
    <span className="tab-more" data-strip-chrome="1" title={`${n} more session${n === 1 ? "" : "s"} — scroll the strip`} aria-hidden="true">
      +{n}
    </span>
  );
}

export function TabBar({ sessions, activeSid, onActivate, onClose, onExport, view, onViewChange }: {
  sessions: Session[];
  activeSid: string | null;
  onActivate: (sid: string) => void;
  onClose: (sid: string) => void;
  onExport: (sid: string) => void;
  view: SessionView;
  onViewChange: (v: SessionView) => void;
}) {
  // P1-5: closing a session kills a possibly-running command — inline
  // two-step confirm, copied from the memory_delete pattern (MemoryPage):
  // first click arms ("close?"), second executes. Cancel disarms.
  const [confirmSid, setConfirmSid] = useState<string | null>(null);
  // Keep the ACTIVE tab on screen: every activation here is programmatic
  // (close selects a neighbour, the AI opens sessions, a deep link selects),
  // and none of those scroll the strip — see the hook's header.
  const tabsRef = useActiveTabVisible(activeSid, sessions.length);
  // The strip hides sessions behind its right edge at any realistic width; say so, and say it only
  // when it is true (see the hook for the measurement that made this necessary).
  // `hidden` is HOW MANY the strip is keeping out of sight — the same measurement that decides the fade,
  // reported instead of discarded. The strip said "there is more" and never how much (round 168).
  const { overflowing: more, hidden } = useStripOverflow(tabsRef, sessions.length);
  // TEN TABS, ONE LABEL. Measured on the live device (2026-09-17): 16 sessions, 10 of them labelled
  // `pwsh`, so the strip rendered ten identical tabs truncated to `pws…` — no way to tell which session
  // was which, in the ONLY surface this density offers for reaching them. The first keeps the bare label
  // (the least surprising change), and repeats take a counter: `pwsh`, `pwsh 2`, `pwsh 3`.
  //
  // The distinguisher is a NUMBER because that is the only information the row actually has — a pty
  // session's label is its shell, its sid is opaque, and two PowerShell sessions ARE interchangeable
  // until you look inside them. A number says exactly that instead of implying a difference.
  // The helper lives in lib/ because the DESKTOP strip needs it too and had been left out (round 170 found
  // `d1, serial:COM4, d1, …` on a rendered page). One implementation, two strips.
  const displayLabel = disambiguateLabels(sessions);
  return (
    <div className="tabrow" data-more={more ? "1" : undefined}>
      <div id="tabs" role="tablist" aria-label="Terminal sessions" ref={tabsRef}>
        {sessions.map((s, tabIndex) => {
          // A question is waiting for a PERSON in this session. Keyed on
          // `pendingApproval`, never on `approvalRequired`: an armed tab asks
          // before every command, so keying off the gate would mark every armed
          // session forever and the mark would stop meaning anything. There is
          // no count here on purpose — a session holds at most one question, so
          // a number would be either 0 or 1 and carry no information.
          const waiting = !s.closed && !!s.pendingApproval;
          return (
          <div
            key={s.sid}
            // role=tab: the div already carried aria-selected, which is only
            // valid on this role — and a name given by aria-label is dropped on
            // a generic element, so the waiting label below needs it too.
            role="tab"
            className={`tab ${s.closed ? "closed" : ""} ${s.sid === activeSid ? "active" : ""}`}
            // The hook finds the active tab by this attribute rather than by
            // id: a session id in a selector needs escaping (`:` / `@` are
            // common) and `CSS.escape` is absent in jsdom.
            data-active={s.sid === activeSid ? "1" : undefined}
            // round-161: closed tabs are visually dead AND honestly labelled —
            // activation rejects closed sessions (round-113 unmounted their
            // panes), so a click was a silent no-op before.
            // The label used to promise the history "stays in Trajectory/Logs":
            // there is no Logs view, and Trajectory shows the ACTIVE session. The
            // durable trail IS reachable now — in the Archive page, the surface
            // that reads this device's recorded sessions — so the tab names where
            // it actually is instead of a place that does not exist.
            title={
              s.closed
                ? `${displayLabel[tabIndex]} — closed (its recorded trail is in Archive)`
                : waiting
                  ? `${displayLabel[tabIndex]} — waiting for your approval`
                  : s.sid
            }
            aria-label={waiting ? `${displayLabel[tabIndex]} — waiting for your approval` : displayLabel[tabIndex]}
            aria-selected={s.sid === activeSid}
            onClick={() => { if (!s.closed) onActivate(s.sid); }}
          >
            {/* ONE MARK, TWO CHANNELS: the silhouette is the session's LIVENESS (lib/liveness.ts) and the lane
                colour tints it. A waiting session used to need a SECOND element beside this dot, because the dot
                could only carry a lane colour — so with sixteen tabs "which one is holding a question" was
                readable only from the aria-label below. `active` is false on purpose: the panel's only activity
                signal is DEVICE-wide (useDeviceActivity), and a halo on all sixteen tabs would say nothing. The
                model takes a per-session signal the day one exists. */}
            <span className="mark tab-dot" data-live={livenessOf({ reachable: !s.closed, pending: waiting, active: false })} data-kind={s.kind} />
            <span className="tab-name">{displayLabel[tabIndex]}</span>
            <span
              className="tab-export"
              title="Export this session log"
              onClick={(e) => { e.stopPropagation(); onExport(s.sid); }}
            >
              <Icon name="export" size={12} />
            </span>
            {!s.savedOnly && !s.closed && (
              confirmSid === s.sid ? (
                <span className="tab-confirm" onClick={(e) => e.stopPropagation()}>
                  <span className="tab-confirm-hint">close?</span>
                  <button
                    type="button"
                    className="btn btn-danger btn-mini"
                    onClick={(e) => { e.stopPropagation(); setConfirmSid(null); onClose(s.sid); }}
                  >Close</button>
                  <button
                    type="button"
                    className="btn btn-ghost btn-mini"
                    onClick={(e) => { e.stopPropagation(); setConfirmSid(null); }}
                  >Cancel</button>
                </span>
              ) : (
                <span
                  className="tab-close"
                  title="Close session"
                  onClick={(e) => { e.stopPropagation(); setConfirmSid(s.sid); }}
                >
                  <Icon name="close" size={12} />
                </span>
              )
            )}
          </div>
          );
        })}
        {/* HOW MANY, NOT JUST "MORE". aria-hidden because the count is a visual affordance for the strip
            itself: a screen reader reaching the tab list already gets every tab, including the hidden
            ones. The title carries the same information for a pointer. */}
        {<StripMore n={hidden} />}
      </div>
      {/* The per-session view switch used to render here (round-admin-ui Task 5). It moved to the
          session control bar in TerminalWorkspace, which builds it once for BOTH densities — this strip
          is the only way to reach a session, and the switch was taking the width the tabs need. */}
    </div>
  );
}
