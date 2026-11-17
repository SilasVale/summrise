import { useEffect, useRef, useState } from "react";
import { Clock } from "./Clock";
import type { LiveLine, StartedCall } from "./liveActivity";
import { sessionActivity } from "./sessionActivity";
import {
  buildSessionSpine,
  groupEnd,
  groupHeader,
  isUnknownRow,
  type SpineGroup,
  type SpineRow
} from "./sessionSpine";
import { LIVE_LIMIT } from "./useLiveActivity";

/** How far the rows may scroll before the reader is treated as reading older activity. */
const SCROLL_AWAY_PX = 8;

/** One call row: the console's own card, unchanged, plus the neutral shape an unknown row takes. */
function RowCard({
  row,
  neutral,
  onJump
}: {
  row: SpineRow;
  neutral: boolean;
  onJump?: (text: string) => void;
}) {
  const tone = neutral ? "unknown" : (row.tone ?? (row.outcome === "failed" ? "failed" : "ok"));
  const title = row.title ?? "Unknown event";
  return (
    <li className={"activity-card tone-" + tone}>
      <div className="activity-card-meta">
        <span className={"chip activity-actor actor-" + row.actor.kind} title={row.actor.title}>
          {row.actor.label}
        </span>
        <Clock ts={row.atMs} />
        {neutral ? (
          // Neutral on purpose: the page cannot say what this row means, only that it is there.
          <span className="activity-card-state state-unknown">unknown event</span>
        ) : tone !== "ok" ? (
          <span className={"activity-card-state state-" + tone}>{tone}</span>
        ) : null}
      </div>
      {row.actor.kind === "session" ? (
        <p className="activity-card-title">{title}</p>
      ) : (
        <code className="activity-card-title">{title}</code>
      )}
      {row.facts !== undefined && row.facts.length > 0 && (
        <p className="activity-card-facts">{row.facts.join(" · ")}</p>
      )}
      {neutral && row.rowKind !== undefined && (
        <p className="activity-card-kind">{row.rowKind}</p>
      )}
      {!neutral && tone !== "running" && row.jump !== undefined && onJump !== undefined && (
        <button type="button" className="mini activity-card-jump" onClick={() => onJump(row.jump!)}>
          Show in terminal
        </button>
      )}
    </li>
  );
}

/**
 * One transaction, as the record describes it.
 *
 * The header is the purpose and its author's claim badge; the body is the rows the device observed;
 * the foot is the receipt's totals or the fact that no end was recorded. The page never closes a
 * working group: only the transaction's author may end it.
 */
function GroupBlock({
  group,
  onJump
}: {
  group: SpineGroup;
  onJump?: (text: string) => void;
}) {
  const header = groupHeader(group);
  const end = groupEnd(group);
  return (
    <li className={"activity-group state-" + group.state}>
      <header className="activity-group-head">
        <span
          className={"chip activity-group-badge" + (header.badge === "unattributed" ? " is-unattributed" : "")}
          title={header.claim}
        >
          {header.badge === "claim" ? "claim" : "no author retained"}
        </span>
        <h3 className="activity-group-purpose">{header.purpose}</h3>
        {header.author !== null && (
          <span className="activity-group-author">
            by <span className="activity-actor-name">{header.author}</span>
          </span>
        )}
        <span className={"activity-card-state state-" + header.state}>{header.state}</span>
      </header>
      <ul className="activity-group-calls">
        {[...group.calls].reverse().map((row) => (
          <RowCard
            key={row.id}
            row={row}
            // The spine's own class, not a second reading of the outcome: an unplaceable row is
            // neutral wherever it is drawn, including inside a group.
            neutral={isUnknownRow(row)}
            onJump={onJump}
          />
        ))}
      </ul>
      <p className={"activity-group-foot" + (end.open ? " is-open" : "")}>{end.sentence}</p>
    </li>
  );
}

export type ActivityRailProps = {
  /** The active tab's device session; only this session's rows are drawn. */
  session: string;
  /** The grant's machine; another machine's calls never enter the rail. */
  deviceId: string;
  /** The console's live lines, newest first, from the same window the terminal card reads. */
  lines: readonly LiveLine[];
  /** Calls the proxy has announced and not cleared. Optional for a test or a page without a feed. */
  running?: readonly StartedCall[];
  /** The feed's state. A page that renders the rail without a feed is still connecting. */
  state?: "connecting" | "live" | "offline";
  /** Lines the console's buffer dropped that no reread has covered yet. */
  lagged?: number;
  /** The operator's names for token ids, so a client is known by its token rather than a guess. */
  tokenNames: ReadonlyMap<string, string>;
  /** Search the active xterm's scrollback for the text a finished write carried. */
  onJump?: (text: string) => void;
};

/**
 * The active session's activity, beside its terminal.
 *
 * This is not a second Activity page: it is the same live window, narrowed to the tab in front. A
 * row is drawn only when the record proves the active device and session (see `sessionActivity`).
 * Calls the record cannot place are counted under the list, never attributed to the tab; no label
 * claims a model or a person, because a token is a credential, not proof of either.
 *
 * The rail is deliberately poorer than a command history: the console streams a 200-line
 * machine-wide window and keeps no per-session record, so the footer states that a call older than
 * the window is absent. A running call has no output or percentage here, because the console has
 * neither; it says running and shows the bounded arguments that were published with it.
 *
 * A finished `terminal.write` with typed input is the one row with a jump: the xterm on this tab
 * already holds the text, and the search addon can show it. The jump is a native button, so Tab
 * reaches it; the list is a named scroll region, so a keyboard reader can scroll the rows too.
 */
export function ActivityRail({
  session,
  deviceId,
  lines,
  running = [],
  state = "live",
  lagged = 0,
  tokenNames,
  onJump
}: ActivityRailProps) {
  const { rows, withheld } = sessionActivity(lines, running, deviceId, session, tokenNames);
  const listRef = useRef<HTMLDivElement | null>(null);
  const newestId = rows.length > 0 ? rows[0].id : null;
  const seenNewest = useRef<string | null>(null);
  const [scrolledAway, setScrolledAway] = useState(false);
  const [newRows, setNewRows] = useState(0);

  // A tab switch is a different session in the same rail: forget the old reading position without
  // remounting the rail (and never the xterms in the stage beside it).
  useEffect(() => {
    seenNewest.current = null;
    setScrolledAway(false);
    setNewRows(0);
    if (listRef.current) listRef.current.scrollTop = 0;
  }, [session, deviceId]);

  // The pill is only honest when the reader is away from the newest row: at the top, a new row is
  // already in front of them. The id is the row's, so a re-render that does not add anything raises
  // nothing.
  useEffect(() => {
    if (
      seenNewest.current !== null &&
      newestId !== null &&
      newestId !== seenNewest.current &&
      scrolledAway
    ) {
      setNewRows((count) => count + 1);
    }
    seenNewest.current = newestId;
  }, [newestId, scrolledAway]);

  const onScroll = () => {
    const element = listRef.current;
    if (!element) return;
    const away = element.scrollTop > SCROLL_AWAY_PX;
    setScrolledAway(away);
    if (!away) setNewRows(0);
  };

  const showNewest = () => {
    const element = listRef.current;
    if (element) element.scrollTop = 0;
    setScrolledAway(false);
    setNewRows(0);
  };

  const spine = buildSessionSpine(rows);
  // The document is oldest first (reading order); the rail draws the newest entry first, and each
  // group's newest call first, because the newest fact is what a person watching wants at the top.
  const entriesNewestFirst = [...spine.entries].reverse();

  const note =
    state === "connecting"
      ? "Connecting to the console's live activity…"
      : state === "offline"
        ? "The live activity connection is offline. The rows below are what this page last read; nothing new can arrive until it reconnects."
        : null;

  return (
    <aside className="activity-rail" aria-label="Session activity">
      <header className="activity-rail-head">
        <h2>Activity</h2>
        <span className={"activity-rail-state-badge is-" + state}>
          <span className="activity-rail-dot" aria-hidden="true" />
          {state}
        </span>
        <span className="activity-rail-count">
          {rows.length} {rows.length === 1 ? "row" : "rows"}
        </span>
        {spine.counts.unknown > 0 && (
          // An unplaceable row is never dropped: the count is how the page admits it cannot say
          // what those rows mean.
          <span className="activity-rail-unknown">{spine.counts.unknown} unknown</span>
        )}
      </header>

      {note !== null && (
        <p className="activity-rail-note" role="status">
          {note}
        </p>
      )}

      <div className="activity-rail-body">
        <div
          ref={listRef}
          className="activity-rail-list"
          role="region"
          aria-label="Activity rows"
          tabIndex={0}
          onScroll={onScroll}
        >
          {rows.length > 0 ? (
            <ul className="activity-rail-cards">
              {entriesNewestFirst.map((entry) => {
                if (entry.kind === "goal") {
                  const { divider } = entry;
                  return (
                    <li
                      key={"goal:" + divider.id}
                      className="activity-divider"
                      role="separator"
                      aria-label={divider.goal === null ? "Goal cleared" : "Goal set"}
                    >
                      <span className="activity-divider-label">
                        {divider.goal === null ? "goal cleared" : "goal set"}
                      </span>
                      <span className="activity-divider-text">
                        {divider.goal?.text ?? "the session has no goal"}
                      </span>
                      <span className="activity-divider-by">
                        by <span className="activity-actor-name">{divider.author.label}</span>
                      </span>
                    </li>
                  );
                }
                if (entry.kind === "txn") {
                  return (
                    <GroupBlock
                      key={"txn:" + (entry.group.txn ?? entry.group.startedAtMs)}
                      group={entry.group}
                      onJump={onJump}
                    />
                  );
                }
                return (
                  <RowCard
                    key={entry.row.id}
                    row={entry.row}
                    neutral={entry.neutral}
                    onJump={onJump}
                  />
                );
              })}
            </ul>
          ) : state === "live" ? (
            <p className="hint activity-rail-empty">
              No activity recorded for this session since this page opened.
            </p>
          ) : null}
        </div>

        {scrolledAway && newRows > 0 && (
          <button type="button" className="activity-rail-new" onClick={showNewest}>
            New activity
          </button>
        )}
      </div>

      <footer className="activity-rail-foot">
        {withheld > 0 && (
          <p className="hint">
            {withheld} {withheld === 1 ? "call" : "calls"} on this machine{" "}
            {withheld === 1 ? "is" : "are"} withheld because{" "}
            {withheld === 1 ? "it names" : "they name"} no session. Running calls that name no
            session are counted too; snapshot rows read back from the audit file carry no session or
            evidence.
          </p>
        )}
        {lagged > 0 && (
          <p className="hint">
            The console dropped {lagged} {lagged === 1 ? "live line" : "live lines"} this page could
            not read; the window has a hole at that point.
          </p>
        )}
        <p className="hint">
          This rail reads the console's live {LIVE_LIMIT}-line machine-wide window. It keeps no
          per-session history; a call older than the window is absent, not reconstructed, and a
          dropped line is never invented.
        </p>
        <p className="hint">
          A group shows the calls this window still holds; a receipt's totals are the device's own,
          and this page never adds to them.
        </p>
      </footer>
    </aside>
  );
}
