import { useCallback, useEffect, useRef, useState } from "react";
import { copyText } from "./copy";
import { message } from "./format";
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
    // A MACHINE THE FLEET SAYS IS DOWN IS NOT ASKED — the read cannot reach it, and the page would
    // sit on "Reading the device's run history…" until the console's dial timed out.
    if (offline) {
      setHistoryNote("The machine is offline; its run history cannot be read until it reconnects.");
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
  }, [detail]);

  const copyCommand = async (text: string) => {
    if (await copyText(text)) setCopied("command");
  };

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
      {detailId && <p className="hint">Reading run {detailId}…</p>}
      {detailError && <p className="error">{detailError}</p>}

      {detail && (
        <section className="card runs-detail" ref={detailRef}>
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
              <button type="button" className="mini" onClick={() => setDetail(null)}>
                Close
              </button>
            </div>
          </header>
          <div className="runs-detail-command">
            <h3>Command</h3>
            <p className="mono">{commandLine(detail)}</p>
          </div>
          <p className="hint">
            This is the record the history kept. The device stores each stream's size, not its
            bytes, so a past run has no stdout or stderr to fetch: output is shown only for a run
            that has just finished under Tools → Run a command, and every earlier run's output is
            gone.
          </p>
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
              <dd>{age(Date.now() - detail.started_at_ms)} ago</dd>
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
                      disabled={detailId !== null}
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
                    <span>{age(Date.now() - row.started_at_ms)} ago</span>
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
