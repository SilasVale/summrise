import { useEffect, useState } from "react";
import type { AuditLine } from "./api";
import { age as elapsed } from "./age";
import { runningAt, type StartedCall } from "./liveActivity";

/** How many ten-second buckets the sparkline draws. */
const BUCKETS = 30;

/** How long a quiet console says "nothing happened" instead of inventing motion. */
const PRESENT_MS = 90_000;

/** Calls per ten-second bucket over the last five minutes, oldest first. */
function buckets(lines: readonly AuditLine[], now: number): number[] {
  const span = BUCKETS * 10_000;
  const counts = new Array<number>(BUCKETS).fill(0);
  for (const line of lines) {
    const back = now - line.ts_ms;
    if (back < 0 || back >= span) continue;
    const index = BUCKETS - 1 - Math.floor(back / 10_000);
    if (index >= 0) counts[index] += 1;
  }
  return counts;
}

/**
 * The machine's "Now" band: is the AI working, and how fast — never the day's calls.
 *
 * Everything here is a fact from the console's own audit: the machine's fetched window and the
 * newest live lines merged, the same window the day line reads. The band reads presence, the last
 * minute's rate and the five-minute sparkline from it; it draws none of its rows, because the calls
 * live in Activity now (audit-41 F1, audit-42 F1). A running call is the one line the band keeps:
 * the proxy announces it before the device answers, so it is not an audit row yet.
 *
 * The console's own reads are facts in that same record, so they are excluded: the band counts the
 * calls a client made, never the console's own polling. When nothing has happened the band says so —
 * a pulse that kept beating through an idle night would be the one thing this page must not do,
 * because the whole point is knowing whether the machine is working.
 */
export function LiveNow(props: {
  /**
   * The same window the day line reads: the machine's fetched lines merged with the stream's,
   * newest first. It is wider than the fleet's newest 200 live lines, which after a reconnect is
   * only the newest forty and could hold none of this machine's calls.
   */
  lines: AuditLine[];
  /**
   * The calls the proxy announced and has not cleared. A started frame is not an audit line, so
   * these are the only proof that a call is running before the device answers; only a client's
   * calls are shown, because the band counts the AI and not the console's own reads.
   */
  running?: StartedCall[];
  state: "connecting" | "live" | "offline";
}) {
  // The sparkline advances on its own clock: a stream of timestamps that never ticks reads as a
  // screenshot, not as something happening.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  // **THE BAND COUNTS THE AI, NOT THE CONSOLE'S OWN POLLING.** The terminal page's ten-second
  // `terminal.list` and the terminal stream's parked `terminal.read` are lines in the same audit, and
  // counting them made an idle machine read "Active now · 6 calls in the last minute". A line with
  // no caller (an old console's, or a snapshot) is not a client's call either, so it is not one here.
  const calls = props.lines.filter((line) => line.caller === "client");
  // A started entry is the proxy's own announcement and is bounded by its own cut: the page applies
  // the same bound so a `finished` frame lost on the way cannot leave the line running forever.
  const running = runningAt(props.running ?? [], now).filter(
    (call) => call.caller === "client"
  );
  const newest = calls[0]?.ts_ms ?? 0;
  const present = running.length > 0 || (newest > 0 && now - newest < PRESENT_MS);
  const lastMinute = calls.filter((line) => now - line.ts_ms < 60_000).length;
  const counts = buckets(calls, now);
  const peak = Math.max(1, ...counts);

  const status =
    props.state === "offline"
      ? "Reconnecting"
      : props.state === "connecting"
        ? "Connecting"
        : present
          ? "Active now"
          : "Quiet";

  return (
    <section className="live-now" aria-labelledby="now-heading" aria-live="off">
      <div className="live-now-head">
        <h2 id="now-heading">Now</h2>
        <span className={"live-state" + (present ? " is-active" : "")}>
          <span className="live-dot" aria-hidden="true" />
          {status}
        </span>
        <span className="live-rate">
          {props.state === "live"
            ? lastMinute === 0
              ? "no calls in the last minute"
              : `${lastMinute} call${lastMinute === 1 ? "" : "s"} in the last minute`
            : props.state === "offline"
              ? "the stream is down; the list below is the last read"
              : "opening the stream"}
        </span>
      </div>

      <svg
        className="live-spark"
        viewBox={`0 0 ${BUCKETS * 4} 32`}
        preserveAspectRatio="none"
        role="img"
        aria-label={`Client calls per ten seconds over the last five minutes; the busiest bucket had ${peak === 1 && counts.every((count) => count === 0) ? 0 : peak}.`}
      >
        {/* **A SILENT FIVE MINUTES IS ONE FLAT LINE, NOT THIRTY GAPS (the hundred-and-forty-fifth list).** Measured on the
            live console 2026-11-12 (`0.3.274-ux198`, 1440, a quiet machine): thirty 3-px buckets at 2 px each drew what read
            as a DASHED PLACEHOLDER under `no calls in the last minute` — a broken chart where the fact was "zero, for five
            minutes". The one honest shape for zero is the axis itself: a single hairline, and the sentence above it. */}
        {counts.every((count) => count === 0) ? (
          <rect x={0} y={30} width={BUCKETS * 4 - 1} height={2} rx={1} className="spark-off" />
        ) : (
          counts.map((count, index) => {
          const height = count === 0 ? 2 : Math.max(3, Math.round((count / peak) * 30));
          return (
            <rect
              key={index}
              x={index * 4}
              y={32 - height}
              width={3}
              height={height}
              rx={1}
              className={count > 0 ? "spark-on" : "spark-off"}
            />
          );
          })
        )}
      </svg>

      {running.length > 0 && (
        <ul className="live-list live-running" aria-label="Running now">
          {running.map((call) => (
            <li key={call.id} className="live-entry live-running-entry">
              <span className="live-entry-dot live-running-dot" aria-hidden="true" />
              <span className="live-sentence">
                running {call.tool} · {elapsed(now - call.at_ms)}
              </span>
            </li>
          ))}
        </ul>
      )}
      {/* **THE DAY LINE IS THE SAME CARD'S SECOND HALF (the hundred-and-sixty-third list).** Measured at 1440: the page drew
          the Now band and the day line as two bordered panels 12 px apart, each holding one sentence — two surfaces for two
          lines, on a page whose lower 600 px were empty. The band's own rule painted `--panel`/`--line`/`--radius` BY HAND
          rather than taking the card atom, which is why the two could not simply be one: the band was a card in everything
          but its class. It takes `.card` now, and the caller puts its day line inside it. */}
    </section>
  );
}
