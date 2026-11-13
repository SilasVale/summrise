import { useCallback, useEffect, useRef, useState } from "react";
import { copyText } from "./copy";
import { Moment } from "./Clock";
import { clockTime, exactMoment, isoDate, message } from "./format";
import { useRequestGate } from "./requests";
import { age } from "./age";
import { callTool, type Grant } from "./terminal";
import {
  HISTORY_LIMIT,
  bytes,
  commandLine,
  exitChip,
  exitSentence,
  shortCommand,
  spellDuration,
  type RunRecord
} from "./runFormat";

/**
 * Runs, as a page: the history of the last runs and the record one kept.
 *
 * THE EXEC FORM LIVES UNDER TOOLS NOW. `#device/tools` is where a command is composed; this page is
 * the history (`runs.list`), the record a row opens (`runs.get`), and the sentences that say what
 * the history does and does not keep. There is no route that makes a run from here, so the history
 * cannot become a second way to run a command.
 *
 * A run is a device call like any other, so it goes through the same grant as the terminal, the
 * browser, memory and monitors. The history keeps no output bodies, so this page never grows an
 * output box for a past run: it says why there is nothing to show instead.
 */
export function RunsPage({
  grant,
  focusRunId = null,
  onFocusConsumed,
  deviceOnline
}: {
  grant: Grant;
  /** A run id carried from Activity; the page opens its record once, on arrival. */
  focusRunId?: string | null;
  onFocusConsumed?: () => void;
  /** The fleet's reachability, as the other device pages take it; `false` is the only fact. */
  deviceOnline?: boolean;
}) {
  const offline = deviceOnline === false;
  const [history, setHistory] = useState<RunRecord[]>([]);
  const [historyNote, setHistoryNote] = useState("Reading the device's run history…");
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [loadingHistory, setLoadingHistory] = useState(false);

  const [detail, setDetail] = useState<RunRecord | null>(null);
  const [detailId, setDetailId] = useState<string | null>(null);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [copied, setCopied] = useState<"command" | null>(null);
  // The record opens on its short command. A 4,590-character `-EncodedCommand` used to be painted
  // whole and push Exit/Duration/Started more than a screen below the card; the full line is one
  // press away, and a different record always starts short again.
  const [fullCommand, setFullCommand] = useState(false);
  const detailRef = useRef<HTMLElement | null>(null);

  // A failed first read has nothing to keep showing; a failed refresh must leave the previous rows
  // where they are, so the two cases are told apart by whether a list ever arrived.
  const listed = useRef(false);
  // Answer ordering belongs to a ticket: the history and one row's record each get their own, because
  // a refresh of the list must not discard a record that is still waiting.
  const historyGate = useRequestGate();
  const detailGate = useRequestGate();
  // The Activity → Runs handoff is consumed once. `show` is re-created every render, so the effect
  // reads the latest one through a ref instead of taking it as a dependency.
  const showRef = useRef<(row: RunRecord) => Promise<void>>(async () => {});
  const focused = useRef<string | null>(null);
  const focusRef = useRef<string | null>(focusRunId);
  const onFocusConsumedRef = useRef(onFocusConsumed);
  focusRef.current = focusRunId;
  onFocusConsumedRef.current = onFocusConsumed;

  const read = useCallback(async () => {
    const ticket = historyGate.begin();
    setLoadingHistory(true);
    setHistoryError(null);
    try {
      const rows = JSON.parse(
        await callTool(grant, "runs.list", { limit: HISTORY_LIMIT })
      ) as RunRecord[];
      if (historyGate.stale(ticket)) return;
      listed.current = true;
      setHistory(rows);
      setHistoryNote(historyNoteFor(rows));
      const target = focusRef.current;
      if (target && focused.current !== target) {
        focused.current = target;
        void showRef.current({ id: target } as RunRecord);
        onFocusConsumedRef.current?.();
      }
    } catch (cause) {
      if (historyGate.stale(ticket)) return;
      if (!listed.current) setHistoryNote("The device's run history could not be read.");
      setHistoryError("Could not read the run history: " + message(cause));
    } finally {
      if (!historyGate.stale(ticket)) setLoadingHistory(false);
    }
  }, [grant, historyGate]);

  // A different grant is a different device's history. It also invalidates a row read that is still
  // waiting: its answer belongs to the device this page has left.
  useEffect(() => {
    detailGate.begin();
    listed.current = false;
    focused.current = null;
    setHistory([]);
    setHistoryNote("Reading the device's run history…");
    setHistoryError(null);
    setDetail(null);
    setDetailId(null);
    setDetailError(null);
    setFullCommand(false);
    // A MACHINE THE FLEET SAYS IS DOWN IS NOT ASKED — the read cannot reach it, and the page would
    // sit on "Reading the device's run history…" until the console's dial timed out.
    if (offline) {
      // The shell notice already states the machine, its last-seen and that nothing on the page
      // can reach it (audit-44 F5); this line names only what is missing.
      setHistoryNote("Its run history cannot be read.");
      return;
    }
    void read();
  }, [read, detailGate, offline]);

  // **A RUN LINK CAN ARRIVE AT A PAGE THAT IS ALREADY OPEN.** The focus used to be consumed only
  // inside a history read, so a hash change to #run/… while Runs was mounted loaded nothing and left
  // the previous sentence on screen. A new id now loads its record directly; the read path keeps its
  // own consumption for the first mount, and `focused` stops the two from loading the same run twice.
  //
  // **IT IS DECLARED AFTER THE DEVICE RESET ON PURPOSE.** That effect begins the gate, which
  // invalidates any read already in flight — correct for a device switch and fatal for this one on a
  // first mount, where the focus effect used to run first and have its answer discarded as stale:
  // an Activity → Runs link opened the history and never the run. Effects run in declaration order,
  // so the reset settles before the link asks for its record.
  useEffect(() => {
    if (!focusRunId || focused.current === focusRunId) return;
    focused.current = focusRunId;
    void showRef.current({ id: focusRunId } as RunRecord);
    onFocusConsumedRef.current?.();
  }, [focusRunId]);

  const show = async (row: RunRecord) => {
    if (detailId !== null) return;
    const ticket = detailGate.begin();
    setDetailId(row.id);
    setDetailError(null);
    try {
      const record = JSON.parse(
        await callTool(grant, "runs.get", { id: row.id })
      ) as RunRecord;
      if (detailGate.stale(ticket)) return;
      setDetail(record);
      setFullCommand(false);
    } catch (cause) {
      if (detailGate.stale(ticket)) return;
      setDetail(null);
      setDetailError("Could not read run " + row.id + ": " + message(cause));
    } finally {
      // A superseded read must not clear the id of the one that replaced it.
      if (!detailGate.stale(ticket)) setDetailId(null);
    }
  };

  showRef.current = show;

  // **THE RECORD IS ABOVE THE LIST, AND ON A PHONE THE LIST IS THE PAGE.** Tapping a card halfway
  // down a 20-run history used to open the record above the viewport: the card kept its tint and
  // the reader saw nothing happen. Whenever a record arrives it is brought into view; on the
  // desktop page the whole record is already visible and `nearest` does not move it. `scrollIntoView`
  // is absent in jsdom and in a runtime that cannot scroll, so the call is optional like the token
  // list's.
  useEffect(() => {
    if (!detail) return;
    detailRef.current?.scrollIntoView?.({ block: "nearest", behavior: "smooth" });
    // **THE RECORD TAKES FOCUS WHEN IT ARRIVES.** The reading line told a screen reader that a read
    // was in flight and the record then appeared silently below it; the row's own button kept focus
    // only because the row now stays focusable (`aria-disabled`, not `disabled`), so the operator's
    // next Tab has somewhere to land. The card is the thing that changed, so it is the thing focused.
    detailRef.current?.focus?.();
  }, [detail]);

  const copyCommand = async (text: string) => {
    if (await copyText(text)) setCopied("command");
  };

  // **THE RECORD SHOWS THE COMMAND'S START, NOT ALL OF IT.** The history row has always done this;
  // the record painted the full line, and a 4,590-character encoded command made the command block
  // 1,190 px of a 1,551 px card at 1440 (1,866 px of 2,299 px at 390) and pushed the facts off the
  // screen. The short form states exactly how much is hidden, the full line stays in `Copy command`,
  // and `Show full command` draws it again. The facts are above the command, so exit/duration/
  // started are readable without scrolling past any command at either width.
  const detailLine = detail ? commandLine(detail) : "";
  const detailShort = shortCommand(detailLine);
  const oneLine = detailLine.replace(/\s+/g, " ").trim();
  const commandShortened = detail !== null && oneLine.length > detailShort.length;
  const shownCommand = commandShortened ? detailShort : detailLine;
  const hiddenCharacters = commandShortened ? detailLine.length - (detailShort.length - 1) : 0;

  return (
    <div className="runs-page">
      <div className="runs-history-head">
        <p className="hint">{historyNote}</p>
        <button
          type="button"
          className="mini"
          disabled={loadingHistory || offline}
          title={offline ? "The machine is offline; the history refreshes when it reconnects" : undefined}
          onClick={() => void read()}
        >
          {loadingHistory ? "Refreshing…" : "Refresh history"}
        </button>
      </div>
      {historyError && <p className="error">{historyError}</p>}
      {detailId && (
        <p className="hint" role="status">
          Reading run {detailId}…
        </p>
      )}
      {detailError && <p className="error">{detailError}</p>}

      {detail && (
        <section className="card runs-detail" ref={detailRef} tabIndex={-1}>
          <header>
            <strong className="mono">{detail.id}</strong>
            <div className="actions">
              <button
                type="button"
                className="mini"
                onClick={() => void copyCommand(commandLine(detail))}
              >
                {copied === "command" ? "Copied" : "Copy command"}
              </button>
              <button
                type="button"
                className="mini"
                onClick={() => {
                  setDetail(null);
                  setFullCommand(false);
                }}
              >
                Close
              </button>
            </div>
          </header>
          {/* THE FACTS COME FIRST. The record exists to answer exit/duration/started, and the
              command can be arbitrarily long; the facts must not sit below it. */}
          <dl className="facts">
            <div>
              <dt>Exit</dt>
              <dd>{exitSentence(detail)}</dd>
            </div>
            <div>
              <dt>Duration</dt>
              <dd>{spellDuration(detail.duration_ms)}</dd>
            </div>
            <div>
              <dt>Started</dt>
              {/* THE START IS AN INSTANT, NOT AN AGE. The record's own `started_at_ms` is painted
                  in the console's local clock (the day `isoDate` writes, the clock `clockTime`
                  writes), the exact moment — local to the second and the UTC instant — is the
                  `title`, and `dateTime` carries the UTC for anything that reads the record instead
                  of looking at it. The age stays beside it as the glance; it may move, the instant
                  may not. */}
              <dd>
                <time
                  dateTime={new Date(detail.started_at_ms).toISOString()}
                  title={exactMoment(detail.started_at_ms)}
                >
                  {isoDate(detail.started_at_ms)} {clockTime(detail.started_at_ms)}
                </time>{" "}
                <span className="hint">· {age(Date.now() - detail.started_at_ms)} ago</span>
              </dd>
            </div>
            <div>
              <dt>Working directory</dt>
              <dd className="mono">{detail.cwd}</dd>
            </div>
            <div>
              <dt>stdout</dt>
              <dd>{bytes(detail.stdout_bytes)}</dd>
            </div>
            <div>
              <dt>stderr</dt>
              <dd>{bytes(detail.stderr_bytes)}</dd>
            </div>
          </dl>
          <div className="runs-detail-command">
            <h3>Command</h3>
            <p className="mono" id="runs-detail-command-text">
              {commandShortened && fullCommand ? detailLine : shownCommand}
            </p>
            {commandShortened && (
              <div className="runs-detail-command-more">
                <button
                  type="button"
                  className="mini"
                  aria-expanded={fullCommand}
                  aria-controls="runs-detail-command-text"
                  onClick={() => setFullCommand((shown) => !shown)}
                >
                  {fullCommand ? "Hide full command" : "Show full command"}
                </button>
                <span className="hint">
                  {fullCommand
                    ? `The full command is ${grouped(detailLine.length)} characters.`
                    : `${grouped(hiddenCharacters)} of ${grouped(detailLine.length)} characters hidden.`}
                </span>
              </div>
            )}
          </div>
          <p className="hint">
            This is the record the history kept. The device stores each stream's size, not its
            bytes, so a past run has no stdout or stderr to fetch: output is shown only for a run
            that has just finished under Tools → Run a command, and every earlier run's output is
            gone.
          </p>
        </section>
      )}

      {history.length > 0 && (
        <div className="scroll runs-history">
          <table>
            <thead>
              <tr>
                <th>Command</th>
                <th>Exit</th>
                <th>Duration</th>
                <th>Started</th>
                <th>stdout / stderr</th>
              </tr>
            </thead>
            <tbody>
              {history.map((row) => (
                <tr
                  key={row.id}
                  className={"runs-row" + (detail?.id === row.id ? " runs-row-active" : "")}
                >
                  <td className="runs-command-cell">
                    <button
                      type="button"
                      className="runs-open"
                      aria-expanded={detail?.id === row.id}
                      aria-disabled={detailId !== null}
                      title={commandLine(row)}
                      onClick={() => void show(row)}
                    >
                      {shortCommand(commandLine(row))}
                    </button>
                  </td>
                  {/* THE PHONE CARD'S LABELS. The table head is hidden at <=639 px, so every fact
                      states what it is beside it; on the desktop these spans are `display: none`
                      and the header above is the only label, exactly as before. */}
                  <td className="runs-fact">
                    <span className="runs-fact-label">Exit</span>
                    <span className={exitChip(row)}>{exitSentence(row)}</span>
                  </td>
                  <td className="runs-fact">
                    <span className="runs-fact-label">Duration</span>
                    <span>{spellDuration(row.duration_ms)}</span>
                  </td>
                  <td className="runs-fact">
                    <span className="runs-fact-label">Started</span>
                    {/* **THE START IS ONE CLOCK, AND ONE GRID CELL ON A PHONE (audit-43 F8).** The
                        row used to show only `31m 11s ago`, then the twenty-second list gave it the
                        Table's clock and age — but as three children of the phone's two-column
                        cell grid, so the age wrapped into the label's column under a dangling `·`
                        and, at 1440, stranded `ago` on its own line. `Moment` keeps the clock and
                        the age in one nowrap wrapper: the grid sees a label and one value, exactly
                        like every other fact, and the exact instant is the `<time>`'s title. */}
                    <Moment
                      ts={row.started_at_ms}
                      age={`${age(Date.now() - row.started_at_ms)} ago`}
                      className="runs-started"
                      ageClassName="hint"
                    />
                  </td>
                  <td className="runs-fact">
                    <span className="runs-fact-label">stdout / stderr</span>
                    <span className="mono">
                      {bytes(row.stdout_bytes)} / {bytes(row.stderr_bytes)}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function grouped(count: number): string {
  return String(count).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

function historyNoteFor(rows: RunRecord[]): string {
  if (rows.length === 0) {
    // `HISTORY_LIMIT` is this page's request, not the device's retention: the device keeps
    // `HISTORY_LINES` (10_000, device/device/src/capabilities/runs.rs) and can answer up to 100
    // rows per read. Saying it keeps "the last 20" was false; the empty state names the request and
    // the page that now makes a run.
    return (
      "No runs yet on this machine. Run one from Tools → Run a command; this page asks for the " +
      `newest ${HISTORY_LIMIT} runs, and the machine keeps more history than that.`
    );
  }
  const noun = rows.length === 1 ? "run" : "runs";
  // The device answers the newest rows up to the limit; only a full answer is worth calling the
  // newest N, because a shorter one is the whole history it had.
  const count =
    rows.length >= HISTORY_LIMIT
      ? `The newest ${HISTORY_LIMIT} ${noun}`
      : `${rows.length} recent ${noun}`;
  return `${count}, newest first. Choose a command to read its record.`;
}
