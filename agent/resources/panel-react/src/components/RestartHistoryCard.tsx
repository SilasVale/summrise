// RestartHistoryCard — HOW OFTEN THIS AGENT RESTARTS, AND HOW EACH RUN ENDED.
//
// WHY THIS EXISTS. Every other surface in this panel answers a question about the present:
// the strip's uptime, its vitals, the boot chip ("how did the run before this one end").
// `/api/status`'s verdict is OVERWRITTEN at each boot, so the one question an operator asks
// when a device misbehaves — "has this been happening, or was that once?" — had no answer
// anywhere, on the device or in the console. That is not hypothetical: d1's 2026-09-13
// incident was "the agent restarted every one to two hours" and could not even be COUNTED
// while it was happening, because each restart erased the trace of the last.
//
// WHAT IT SHOWS, in the order an operator reads it:
//   * the last 24 hours as a COUNT (restarts, and how many were crashes) — the pattern;
//   * then each recorded boot, newest first, with the device's own sentence on the hover —
//     the evidence behind the count, never a summary the reader cannot check.
//
// HONEST EMPTIES, this panel's discipline: a device that has not booted a history-recording
// build draws ONE quiet sentence saying so (not an empty list that reads as a broken
// feature), and a device that did not answer says THAT — the two are different facts and
// this card never collapses them.
import { useState } from "react";
import { bootKindLabel, isCrash } from "../lib/bootNotice";
import type { BootHistory, BootRecord } from "../hooks/useBootHistory";

/** Rows rendered before the list is folded. A device that restarts hourly has 24 of them in
 *  a day and the card is in Settings, not a log viewer — the count above is the headline and
 *  the rest are one click away. */
const VISIBLE = 6;

const clock = (ms: number): string =>
  new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

const plural = (n: number, one: string, many: string): string =>
  `${n} ${n === 1 ? one : many}`;

/** `2h 04m`, `12m 30s`, `45s` — the same shapes the strip's uptime uses, so two readings of
 *  the same quantity cannot look like two different quantities. */
function fmtSpan(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return h >= 24 ? `${Math.floor(h / 24)}d ${h % 24}h` : `${h}h ${String(m).padStart(2, "0")}m`;
}

function Row({ rec }: { rec: BootRecord }) {
  return (
    <li className="restart-row" data-kind={rec.kind ?? ""} title={rec.detail}>
      <span className="restart-row-time">{clock(rec.tsMs)}</span>
      <span className={`restart-row-kind${isCrash(rec.kind) ? " is-crash" : ""}`}>
        {bootKindLabel(rec.kind)}
      </span>
      {/* What the run before it did with its life. Absent for a first start, where there was
          no run to measure — drawn as nothing, never as "0s". */}
      {rec.uptimeSecs !== null && (
        <span className="restart-row-fact">ran {fmtSpan(rec.uptimeSecs)}</span>
      )}
      {rec.gapSecs !== null && rec.gapSecs > 5 && (
        <span className="restart-row-fact">down {fmtSpan(rec.gapSecs)}</span>
      )}
      {rec.release && <span className="restart-row-rel">{rec.release}</span>}
    </li>
  );
}

/** PURE: the history is handed in by the shell that polls it (see the module header of
 *  `useBootHistory`) — one poller for the panel, and a card a test can render with any
 *  history at all. */
export function RestartHistoryCard({
  history,
  failed,
}: {
  history: BootHistory;
  failed?: boolean;
}) {
  const { boots, summary } = history;
  const [showAll, setShowAll] = useState(false);

  return (
    <div className="settings-section">
      <h2>Restarts</h2>
      {failed && boots.length === 0 ? (
        <p className="muted">
          The device did not answer, so its restart history could not be read. That is not
          the same as a device that has never restarted.
        </p>
      ) : boots.length === 0 ? (
        <p className="muted">
          No restarts recorded yet. This history starts with the first boot of the build that
          keeps it, so a device that has been up since then shows nothing here — which is a
          fact about the record, not about the device.
        </p>
      ) : (
        <>
          <p
            className={`restart-summary${summary.crashes > 0 ? " has-crashes" : ""}`}
            data-crashes={summary.crashes}
          >
            {plural(summary.boots, "restart", "restarts")} in the last{" "}
            {Math.round(summary.windowSecs / 3600)}h
            {summary.crashes > 0
              ? ` — ${plural(summary.crashes, "of them a crash", "of them crashes")}`
              : ", none of them a crash"}
            .
          </p>
          <ul className="restart-list">
            {(showAll ? boots : boots.slice(0, VISIBLE)).map((b) => (
              <Row key={b.tsMs} rec={b} />
            ))}
          </ul>
          {boots.length > VISIBLE && (
            <button type="button" className="btn btn-ghost" onClick={() => setShowAll((v) => !v)}>
              {showAll ? "show fewer" : `show all ${boots.length}`}
            </button>
          )}
          <p className="muted restart-note">
            Each row is a start of this agent. The verdict beside it describes the run that
            came BEFORE it — the same sentence the device writes to its startup log — and the
            full text is on the hover.
          </p>
        </>
      )}
    </div>
  );
}
