import { useEffect, useState } from "react";
import type { Device } from "./api";
import { age, shortId } from "./format";
import type { LiveLine } from "./liveActivity";

/** How many entries the band shows: enough to read the last few steps, not a second Activity page. */
const SHOWN = 5;

/** How many ten-second buckets the sparkline draws. */
const BUCKETS = 30;

/** How long a quiet console says "nothing happened" instead of inventing motion. */
const PRESENT_MS = 90_000;

function deviceName(devices: Device[], id: string | undefined): string | null {
  if (!id) return null;
  const device = devices.find((candidate) => candidate.id === id);
  return device?.name ?? shortId(id);
}

/** Calls per ten-second bucket over the last five minutes, oldest first. */
function buckets(lines: LiveLine[], now: number): number[] {
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
 * The hub's "Now" band: what the AI is doing, as it does it.
 *
 * Everything here is a fact from the console's own audit stream. The console's reads are facts in
 * that same stream, so they are excluded: the band counts the calls a client made, never the
 * console's own polling. When nothing has happened the band says so — a pulse that kept beating
 * through an idle night would be the one thing this page must not do, because the whole point is
 * knowing whether the machine is working.
 */
export function LiveNow(props: {
  lines: LiveLine[];
  devices: Device[];
  state: "connecting" | "live" | "offline";
  onAll: () => void;
  /** Open a machine's Terminal tab, where an AI's session can be watched. */
  onWatch?: (deviceId: string) => void;
}) {
  // The age column and the sparkline advance on their own clock: a stream of timestamps that never
  // ticks reads as a screenshot, not as something happening.
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
  const newest = calls[0]?.ts_ms ?? 0;
  const present = newest > 0 && now - newest < PRESENT_MS;
  const lastMinute = calls.filter((line) => now - line.ts_ms < 60_000).length;
  const shown = calls.slice(0, SHOWN);
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
        <button type="button" className="link live-all" onClick={props.onAll}>
          All activity <span aria-hidden="true">→</span>
        </button>
      </div>

      <svg
        className="live-spark"
        viewBox={`0 0 ${BUCKETS * 4} 32`}
        preserveAspectRatio="none"
        role="img"
        aria-label={`Client calls per ten seconds over the last five minutes; the busiest bucket had ${peak === 1 && counts.every((count) => count === 0) ? 0 : peak}.`}
      >
        {counts.map((count, index) => {
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
        })}
      </svg>

      {shown.length === 0 ? (
        <p className="hint live-empty">
          {props.state === "live"
            ? "No client calls yet. When an AI client calls this console, its work appears here; the console's own reads are not counted."
            : "The activity stream is not open, so nothing can appear here yet."}
        </p>
      ) : (
        <ol className="live-list">
          {shown.map((line) => {
            const machine = deviceName(props.devices, line.device_id);
            return (
              <li key={`${line.ts_ms}-${line.tool ?? line.kind}`} className="live-entry">
                <span
                  className="live-entry-dot"
                  data-tone={line.ok === false || line.status >= 400 ? "bad" : "ok"}
                  aria-hidden="true"
                />
                <span className="live-sentence">
                  {line.sentence}
                  {machine && <span className="live-machine"> on {machine}</span>}
                </span>
                {typeof line.ms === "number" && <span className="live-ms">{line.ms} ms</span>}
                {line.tool?.startsWith("terminal.") && line.device_id && props.onWatch && (
                  <button
                    type="button"
                    className="live-watch"
                    title="Open this machine's Terminal and watch the session"
                    onClick={() => props.onWatch?.(line.device_id as string)}
                  >
                    Watch
                  </button>
                )}
                <span className="live-age" title={new Date(line.ts_ms).toLocaleString()}>
                  {age(line.ts_ms)}
                </span>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
