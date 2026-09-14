// MonitorsCard — "is that host up, and since when?", as an instrument.
//
// WHY IT EXISTS. Watching a host meant a hand-rolled loop in a terminal session: a
// `Test-NetConnection` every ten seconds, printing timestamps. That loop dies with the session,
// says nothing while nobody is reading it, and cannot be shown to anyone afterwards. The device
// now keeps the watch (see `agent/src/monitor.rs`) and this card is where a human reads it.
//
// WHAT IT SHOWS, per target:
//   * the CURRENT state and WHEN IT STARTED — "down since 18:41" is the whole story, and it is
//     the number an operator reads first;
//   * a latency chart with GAPS for failed probes (the same rule as the vitals sparkline: a
//     failed probe is not a slow one, and a line bridged across it would assert a measurement
//     nobody took);
//   * the share of probes that answered and the latency range, in words, beside the shape;
//   * a remove control and a "check now" — the two things an operator wants on a list they own.
//
// HONEST EMPTIES: a target with no probes yet, and a device that did not answer, are different
// sentences. Neither draws an empty chart that reads as a dead host.
import { useState } from "react";
import { Sparkline } from "./Sparkline";
import { fmtSince, type MonitorTarget, type Monitors } from "../hooks/useMonitors";

/** A wall-clock stamp for the log — `18:41:07`. Local time, because the operator reading it is
 *  standing next to the device, and a log they cannot line up with their own clock is a log they
 *  have to convert before they can use it. */
function clock(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

const pct = (v: number): string => `${Math.round(v)}%`;

function TargetRow({
  target,
  nowMs,
  onRemove,
  onProbe,
}: {
  target: MonitorTarget;
  nowMs: number;
  onRemove: (id: string) => void;
  onProbe: (id: string) => void;
}) {
  const { summary, series, transitions, path: httpPath } = target;
  // The URL an HTTP check is actually asking, which is the thing an operator copies into a browser
  // when the device says it is down.
  const subject = `${target.host}:${target.port}${httpPath ?? ""}`;
  const down = summary.upNow === false;
  const up = summary.upNow === true;
  const state = up ? "up" : down ? "down" : "unknown";
  const sinceMs = nowMs - (summary.sinceMs ?? series[0]?.tsMs ?? nowMs);
  // Latency for the chart: only the probes that ANSWERED. A failed probe becomes a gap, which
  // is exactly what the sparkline's segment rule draws.
  const latency = series.map((p) => (p.ok ? p.ms : null));
  const label =
    summary.probes === 0
      ? `${subject} — no readings yet`
      : `${subject} is ${state}` +
        (summary.lastStatus !== null ? ` (HTTP ${summary.lastStatus})` : "") +
        (summary.latency ? `, ${summary.latency.avg} ms average` : "") +
        `, ${summary.up} of ${summary.probes} probes answered`;

  return (
    <li className="monitor-row" data-state={state} title={label}>
      <div className="monitor-head">
        <span className={`monitor-dot ${state}`} aria-hidden="true" />
        <span className="monitor-name">{subject}</span>
        {summary.lastStatus !== null && (
          // The status is shown EVEN WHEN UP, because `404 up` and `200 up` are different facts
          // about the same service, and hiding the number would make the verdict unfalsifiable.
          <span className={`monitor-status ${summary.lastStatus >= 500 ? "is-bad" : ""}`}>
            HTTP {summary.lastStatus}
          </span>
        )}
        <span className={`monitor-state ${state}`}>
          {up ? "up" : down ? `down ${fmtSince(sinceMs)}` : "no readings"}
        </span>
        <button type="button" className="btn btn-ghost monitor-btn" onClick={() => onProbe(target.id)}>
          check now
        </button>
        <button
          type="button"
          className="btn btn-ghost monitor-btn"
          title="Stop watching this target"
          onClick={() => onRemove(target.id)}
        >
          remove
        </button>
      </div>
      {/* THE OUTAGE LOG — what the chart cannot say in words: when it fell, when it came back,
          and how long each lasted. Newest first, because the last one is what the operator is
          usually asking about. */}
      {transitions.length > 0 && (
        <ul className="monitor-log">
          {[...transitions]
            .reverse()
            .slice(0, 5)
            .map((t, i) => (
              <li key={`${t.atMs}-${i}`} className={t.up ? "is-up" : "is-down"}>
                <span className="monitor-log-time">{clock(t.atMs)}</span>
                <span className="monitor-log-what">
                  {t.up ? "back up" : "went down"}
                  {" — "}
                  {t.up
                    ? `after ${fmtSince(t.lastedMs)} down`
                    : `it had been up ${fmtSince(t.lastedMs)}`}
                </span>
              </li>
            ))}
        </ul>
      )}

      <div className="monitor-body">
        <Sparkline
          values={latency}
          label={label}
          tone="mem"
          // "Nothing probed yet" and "probed, never answered" are different facts; the row's
          // own count of failed probes sits beside this text, and they must not contradict.
          emptyLabel={series.length === 0 ? "no readings yet" : "never answered"}
        />
        <span className="monitor-facts">
          {summary.upPct !== null && (
            <span className="monitor-fact" title="share of the probes that answered">
              {pct(summary.upPct)} up
            </span>
          )}
          {summary.latency && (
            <>
              <span className="monitor-fact">avg {summary.latency.avg} ms</span>
              <span className="monitor-fact">max {summary.latency.max} ms</span>
            </>
          )}
          {summary.down > 0 && (
            <span className="monitor-fact monitor-fact-down">
              {summary.down} failed {summary.down === 1 ? "probe" : "probes"}
            </span>
          )}
          {/* How many times it FELL, which is the number that separates "down" from
              "unstable" — a link that is up every time the operator looks still has a count. */}
          {(summary.drops ?? 0) > 0 && (
            <span className="monitor-fact" title="times it fell from up to down in this window">
              {summary.drops} {summary.drops === 1 ? "drop" : "drops"}
            </span>
          )}
        </span>
      </div>
    </li>
  );
}

export function MonitorsCard({
  monitors,
  failed,
  onAdd,
  onRemove,
  onProbe,
  nowMs = Date.now(),
}: {
  monitors: Monitors;
  failed?: boolean;
  onAdd: (host: string, port: number, path?: string) => Promise<{ ok: boolean; error?: string }>;
  onRemove: (id: string) => void;
  onProbe: (id: string) => void;
  nowMs?: number;
}) {
  const [host, setHost] = useState("");
  const [port, setPort] = useState("22");
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function submit() {
    setBusy(true);
    setError("");
    const n = Number(port);
    const res = await onAdd(host, Number.isFinite(n) ? n : 0, path);
    setBusy(false);
    // The device's reason is shown VERBATIM: it is written for this form ("a host is required",
    // "a port is required…"), and paraphrasing it here would be a second, worse copy.
    if (res.ok) setHost("");
    else setError(res.error ?? "the device refused it");
  }

  const down = monitors.targets.filter((t) => t.summary.upNow === false).length;

  return (
    <div className="settings-section">
      <h3>Reachability</h3>
      {failed && monitors.targets.length === 0 ? (
        <p className="muted">
          The device did not answer, so its watch list could not be read. That is not the same as
          an empty list.
        </p>
      ) : (
        <>
          <p className="muted monitor-lede">
            This device probes each target over TCP on its own timer
            {monitors.intervalSecs ? ` (every ${monitors.intervalSecs}s)` : ""} and keeps the last{" "}
            {monitors.seriesMax ? `${monitors.seriesMax} readings` : "readings"} — so the answer
            survives the session, the terminal and the panel being closed.
            {down > 0 && (
              <>
                {" "}
                <strong>
                  {down} of {monitors.targets.length} are down right now.
                </strong>
              </>
            )}
          </p>

          {monitors.targets.length > 0 && (
            <ul className="monitor-list">
              {monitors.targets.map((t) => (
                <TargetRow
                  key={t.id}
                  target={t}
                  nowMs={nowMs}
                  onRemove={onRemove}
                  onProbe={onProbe}
                />
              ))}
            </ul>
          )}

          <div className="monitor-add">
            <input
              aria-label="host"
              placeholder="192.168.1.1"
              value={host}
              onChange={(e) => setHost(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && void submit()}
            />
            <input
              aria-label="port"
              className="monitor-port"
              placeholder="22"
              value={port}
              onChange={(e) => setPort(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && void submit()}
            />
            {/* OPTIONAL PATH: filling it in turns the check into a real HTTP GET and gives the
                row a status code — the difference between "the port answers" and "the UI works". */}
            <input
              aria-label="path"
              className="monitor-path"
              placeholder="/ (optional HTTP path)"
              title="Optional: GET this path and record the status code instead of only connecting to the port"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && void submit()}
            />
            <button type="button" className="btn" disabled={busy} onClick={() => void submit()}>
              watch
            </button>
          </div>
          {error && <p className="monitor-error">{error}</p>}
        </>
      )}
    </div>
  );
}
