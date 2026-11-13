import { useCallback, useLayoutEffect, useRef, useState } from "react";
import type { Device } from "./api";
import { clockTime, exactMoment, lastHeardMs } from "./format";
import type { FleetRead } from "./HomePage";
import type { LiveLine } from "./liveActivity";
import { DEVICE_TABS, DEVICE_TAB_LABELS, type DeviceTab } from "./Rail";
import { DevicePicker } from "./DevicePicker";
import { OfflineNotice } from "./OfflineNotice";
import type { Grant } from "./terminal";

/**
 * A tab edge this close to the strip's edge is not a hidden tab. `bring()` can leave the active
 * tab one or two pixels past the band, and a chevron drawn over it would cover the tab it names.
 */
const EDGE_SLACK = 8;

/**
 * Device is ONE destination with five tabs, not five rail items. The shell owns the destination's one
 * `h1` (the tab's name) and the tab bar in the same head; the machine identity lives in the header
 * control above, so no page inside states it again. The fragment names the tab, which is why a reload
 * and an old `#files` bookmark both land on the same view.
 *
 * WITH NO MACHINE THE PAGE IS THE PICKER. The tabs describe surfaces that cannot run until a machine
 * is chosen, so drawing them made the page claim to be `Terminal` while its real question — which
 * machine? — sat below the strip (audit-40 F7). The `h1` names the question and the strip and Connect
 * wait for an answer.
 */
export function DevicePageShell({
  tab,
  onTab,
  devices,
  chosen,
  onChoose,
  onConnect,
  fleetRead,
  onRetryFleet,
  journalLines,
  children
}: {
  tab: DeviceTab;
  onTab: (tab: DeviceTab) => void;
  devices: Device[];
  chosen: Device | null;
  onChoose: (id: string) => void;
  onConnect: () => void;
  /** The fleet read's state, so the picker can tell reading, failed and empty apart. */
  fleetRead?: FleetRead;
  /** The page's one fleet Retry, shared with Home's notice. */
  onRetryFleet?: () => void;
  /**
   * The live feed, or this machine's slice of it. The strip keeps only watched session rows for the
   * chosen machine, so a caller cannot put another machine's journal on this page.
   */
  journalLines?: readonly LiveLine[];
  children: (grant: Grant) => React.ReactNode;
}) {
  const grant = grantFor(chosen);
  const journal = chosen ? deviceJournal(journalLines ?? [], chosen.id) : [];
  const tabStrip = useRef<HTMLElement>(null);
  const activeTab = useRef<HTMLButtonElement>(null);
  const [edges, setEdges] = useState({ left: false, right: false });

  // Which edges hold a tab the band cannot show. The strip is the scroll container; its own
  // `scrollWidth`/`clientWidth` say how much travel exists, and `scrollLeft` says where the band is.
  const syncEdges = useCallback(() => {
    const strip = tabStrip.current;
    if (!strip) return;
    const left = strip.scrollLeft > EDGE_SLACK;
    const right = strip.scrollWidth - strip.clientWidth - strip.scrollLeft > EDGE_SLACK;
    setEdges((previous) =>
      previous.left === left && previous.right === right ? previous : { left, right }
    );
  }, []);

  // **THE STRIP ALWAYS SHOWS THE TAB THE ROUTE CHOSE (audit-41 F2).** At 390 the seven labels are
  // 447 px inside a 358 px strip; `#device/machine` selected `Machine` at x=389..461 with
  // `scrollLeft 0`, so the page showed no selected tab at all. The strip scrolls — that is what a
  // scrollable tab bar must do — and the selection is brought into the band on mount, on a route
  // change, and when a resize makes the band narrower. The boxes are measured because the tab's
  // position already includes the strip's scroll; nothing moves when the tab is inside the band.
  //
  // **THE HIDDEN EDGE SAYS IT IS THERE (audit-44 F2).** Bringing `Machine` into the band leaves
  // `Overview` and `Screen` entirely off the left edge, and the strip looked as if it began at
  // `Terminal`; the last tabs are cut at the right edge on other routes. The strip now draws a
  // chevron on each edge that has tabs beyond it, and the chevron scrolls a page of tabs back. The
  // affordance follows `scrollLeft`/`scrollWidth`, so it appears on either side only when that side
  // really holds a tab. A few pixels of slack are not a hidden tab: `bring()` can leave the active
  // tab's edge a pixel or two past the band, and a chevron over it would cover the tab it names.
  useLayoutEffect(() => {
    const strip = tabStrip.current;
    const current = activeTab.current;
    if (!strip || !current) return;
    const bring = () => {
      const band = strip.getBoundingClientRect();
      const tab = current.getBoundingClientRect();
      // **THE CHEVRON IS DRAWN OVER THE BAND, SO IT MUST NOT COVER THE ACTIVE TAB (the hundred-and-eighty-ninth list).**
      // Measured at 390 x 844 with `Memory` selected: the chip's right edge sat under the right chevron and the label read
      // `Memor›` — a tab the page had just brought into view, hidden by the control that brought it. The scroll keeps clear of the
      // chevron's width when one is shown, and falls back to the plain edge once nothing is to the right.
      const nudge = (strip.parentElement?.querySelector<HTMLElement>(".device-tabs-nudge.right")?.offsetWidth ?? 0);
      if (tab.left < band.left) strip.scrollLeft -= band.left - tab.left;
      else if (tab.right > band.right - nudge && nudge > 0) strip.scrollLeft += tab.right - (band.right - nudge);
      else if (tab.right > band.right) strip.scrollLeft += tab.right - band.right;
      syncEdges();
    };
    bring();
    // The first pass runs before the chevron exists (it is drawn by `syncEdges`); the frame after it, `bring` can measure it.
    const settled = window.requestAnimationFrame(() => bring());
    window.addEventListener("resize", bring);
    strip.addEventListener("scroll", syncEdges, { passive: true });
    return () => {
      window.cancelAnimationFrame(settled);
      window.removeEventListener("resize", bring);
      strip.removeEventListener("scroll", syncEdges);
    };
  }, [tab, chosen, syncEdges]);

  // One page of tabs, or one tab's width when the strip is narrower than that: enough to reach the
  // tabs the chevron announced without jumping past them.
  const nudge = (direction: -1 | 1) => {
    const strip = tabStrip.current;
    if (!strip) return;
    const step = Math.max(80, Math.round(strip.clientWidth * 0.8));
    const furthest = Math.max(0, strip.scrollWidth - strip.clientWidth);
    strip.scrollLeft = Math.max(0, Math.min(furthest, strip.scrollLeft + direction * step));
    syncEdges();
  };
  return (
    <div className="page device-page">
      <header className="page-head device-head">
        {/* **ONLY THE TABBED HEAD TAKES THE CONTEXT RUNG (the hundred-and-fifty-eighth list).** The contract's
            `head.nameWhenRepeated` is conditional — a name a CONTROL in the same head already says. When a machine is
            chosen this head carries the tab strip, whose selected tab IS the name; when none is, the head is `Choose a
            machine` with no strip and nothing repeating it, so it keeps the title's rung. Measured live 2026-11-12: the
            demotion had been applied unconditionally and the PICKER's only sentence became a 13 px grey caption. */}
        <h1 className={chosen ? "as-context" : undefined}>
          {chosen ? DEVICE_TAB_LABELS[tab] : "Choose a machine"}
        </h1>
        {chosen && (
          <div className="device-tabs-wrap">
            {edges.left && (
              <button
                type="button"
                className="device-tabs-nudge left"
                aria-label="Show earlier device views"
                title="Earlier tabs"
                onClick={() => nudge(-1)}
              >
                <span aria-hidden="true">‹</span>
              </button>
            )}
            <nav className="device-tabs" aria-label="Device views" ref={tabStrip}>
              {DEVICE_TABS.map((entry) => (
                <button
                  key={entry}
                  type="button"
                  className="device-tab"
                  aria-current={entry === tab ? "page" : undefined}
                  ref={entry === tab ? activeTab : undefined}
                  onClick={() => onTab(entry)}
                >
                  {DEVICE_TAB_LABELS[entry]}
                </button>
              ))}
            </nav>
            {edges.right && (
              <button
                type="button"
                className="device-tabs-nudge right"
                aria-label="Show later device views"
                title="Later tabs"
                onClick={() => nudge(1)}
              >
                <span aria-hidden="true">›</span>
              </button>
            )}
          </div>
        )}
        {/* **ONE ACTION, ONE NAME, ONE HOME.** The six tabs said `Connect` here while Overview
            drew `Connect a client` in its own content: the same sheet, two names and two places.
            The page head is the one place on every tab, and its visible label is the sheet's own
            name. Below 640 px the same button stays in the head's first row. With no machine there
            is no client to connect, so the door waits for the choice. */}
        {chosen && (
          <div className="page-actions device-connect">
            <button type="button" className="mini" onClick={onConnect}>
              Connect a client
            </button>
          </div>
        )}
      </header>

      {/* THE MACHINE'S STATE BELONGS TO EVERY TAB, not only the one that was written first.
          Terminal lives outside this shell (it stays mounted across destinations), so it carries its
          own notice; rendering this one there too would stack two copies of the same sentence. */}
      {chosen && !chosen.online && tab !== "overview" && tab !== "terminal" && (
        <OfflineNotice
          lastSeenMs={lastHeardMs(chosen)}
          detail="Nothing on this page can reach it until it reconnects; the console keeps trying on its own."
        />
      )}

      {/* **THE MACHINE'S OWN JOURNAL, ON EVERY TAB (the live strip).** The console relays a device's
          session and plugin events as they happen; this is the selected machine's slice, newest
          first, so a session opening is visible where the machine is, not only on Activity. It is
          a card like every block under the head, and its list scrolls instead of growing the page:
          twenty rows stay a band, not a second page above the one that was asked for. The chip is
          the stream's own fact — a line arrived for this machine this page opened — not a
          connectivity claim, which the machine header and the offline notice already own. */}
      {chosen && (
        <section className="card device-events" aria-labelledby="device-events-title">
          <header>
            <strong id="device-events-title">Events</strong>
            {journal.length > 0 && <span className="chip chip-ok">Live</span>}
          </header>
          {journal.length === 0 ? (
            <p className="hint">No sessions or plugin events yet on this machine.</p>
          ) : (
            <ol className="device-events-list">
              {journal.map((line) => (
                <li
                  className="device-event"
                  key={`${line.device_id ?? ""}:${line.ts_ms}:${line.sentence}`}
                >
                  <time
                    className="device-event-time"
                    dateTime={new Date(line.ts_ms).toISOString()}
                    title={exactMoment(line.ts_ms)}
                  >
                    {clockTime(line.ts_ms)}
                  </time>
                  <span className="device-event-sentence">{line.sentence}</span>
                  {!sentenceNamesKind(line.sentence, line.kind) && (
                    <span className="device-event-kind">{line.kind}</span>
                  )}
                </li>
              ))}
            </ol>
          )}
        </section>
      )}

      {chosen && grant ? (
        children(grant)
      ) : (
        /* **THE CONTENT IS NOT AN EMPTY STATE (the eighty-third list).** Measured 2026-11-10 at 1,440: this
           page drew `.empty-state` as its BODY in all three of its states — four machines, none, and a failed
           read were the same element in the same slot — so the class that means "there is nothing here"
           wrapped the one thing the page HAS. Rule 19 asks each state to tell the truth; a state whose
           element cannot tell "four" from "none" is the wrong element.
           The picker is a CARD now, the product's own block (radius 14, pad 16, gap 8), and its list fills it
           — measured before, the rows were 326 of a 1,072 px card, because a `ul` sized to its content sat
           inside a card that fills the column, while every other list in the product fills its card (the
           fleet table's row measures 1,070 of 1,072). */
        <section className="card device-picker-card">
          <DevicePicker
            devices={devices}
            onChoose={onChoose}
            fleetRead={fleetRead}
            onRetry={onRetryFleet}
          />
        </section>
      )}
    </div>
  );
}

/**
 * The machine's own journal: how many watched session rows the strip keeps.
 *
 * The live feed holds two hundred lines, most of them calls; a Device strip is a glance, not the
 * record. Twenty rows is the newest work on this machine, and the list scrolls rather than growing.
 */
const JOURNAL_LIMIT = 20;

/**
 * This machine's watched journal rows, newest first.
 *
 * `useLiveActivity` records a device event as a `session` line with `live: true`; a snapshot line
 * was read back from the audit and has no `live`, so it belongs to the record, not this strip. A
 * call is a line too, and an `mcp` one — the machine's own call is Activity's row, not a session
 * this strip watches — which is why the kind matters as much as the machine.
 */
export function deviceJournal(
  lines: readonly LiveLine[],
  deviceId: string | null | undefined
): LiveLine[] {
  if (!deviceId) return [];
  return lines
    .filter(
      (line) => line.live === true && line.kind === "session" && line.device_id === deviceId
    )
    .sort((a, b) => b.ts_ms - a.ts_ms)
    .slice(0, JOURNAL_LIMIT);
}

/** Whether the console's sentence already names the row's kind (`session.idle on …`). */
function sentenceNamesKind(sentence: string, kind: string): boolean {
  return sentence.toLowerCase().includes(kind.toLowerCase());
}

function grantFor(chosen: Device | null): Grant | null {
  return chosen ? { deviceId: chosen.id, envelope: null } : null;
}
