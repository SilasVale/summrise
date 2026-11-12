import { useCallback, useEffect, useRef, useState } from "react";
import { deviceStateFailure, readDeviceState, type DeviceSelf } from "./stateApi";
import type { Grant } from "./terminal";

/**
 * **WHAT THE MACHINE SAYS ABOUT ITSELF (the hundred-and-seventy-seventh list).**
 *
 * Every other line on this page is the console's claim about the machine — its last frame, its last seen, its version from the
 * fleet. `state.get` is the MACHINE's claim: its health, its release and pid, whether its own state document is readable, and the
 * last ten events it recorded. When the two disagree (a live process behind a wedged channel is the measured case), this is the
 * half a person needs, and it was one of the tools the console never called.
 *
 * **A POISONED DOCUMENT IS DRAWN, NOT THROWN.** The device answers `state: null` with `status: "degraded"` rather than failing the
 * call, and that is exactly the state worth seeing.
 */
export function DeviceSelfView(props: { grant: Grant; online?: boolean }) {
  const [self, setSelf] = useState<DeviceSelf | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [reading, setReading] = useState(false);
  const generation = useRef(0);

  const read = useCallback(async () => {
    const mine = ++generation.current;
    setReading(true);
    setFailed(null);
    try {
      const answer = await readDeviceState(props.grant);
      if (generation.current !== mine) return;
      setSelf(answer);
    } catch (error) {
      if (generation.current !== mine) return;
      setFailed(deviceStateFailure(error));
      setSelf(null);
    } finally {
      if (generation.current === mine) setReading(false);
    }
  }, [props.grant]);

  // **READ ONCE WHEN THE TAB IS SHOWN, NOT ON A TIMER.** A `<details>` toggle is not reliably observable across the test
  // environment and a browser, and the honest requirement is "one read per visit": the record is a diagnostic, and `Read again`
  // is how a person asks for a newer one.
  useEffect(() => {
    // A machine the fleet says is away is not asked: the request could only time out (the forty-third list's rule).
    if (props.online === false) return;
    void read();
  }, [read, props.online]);

  return (
    <details className="card-details self-view">
      <summary>The machine's own record</summary>
      {failed !== null && (
        <div className="notice" role="status">
          <span>{failed}</span>
          <button type="button" disabled={reading} onClick={() => void read()}>
            {reading ? "Retrying…" : "Retry"}
          </button>
        </div>
      )}
      {props.online === false && self === null && failed === null && (
        <p className="hint">The machine is offline, so its own record cannot be read.</p>
      )}
      {reading && self === null && failed === null && (
        <p className="hint" role="status">
          Reading the machine's own record…
        </p>
      )}
      {self !== null && (
        <>
          <p className="hint">
            <span className={self.status === "ok" ? "ok" : undefined}>{self.status}</span>
            {self.release !== null ? ` · release ${self.release}` : ""}
            {self.pid !== null ? ` · pid ${self.pid}` : ""}
            {self.poisoned ? " · its state document could not be read" : ""}
            <button type="button" className="link self-refresh" disabled={reading} onClick={() => void read()}>
              {reading ? "Reading…" : "Read again"}
            </button>
          </p>
          {self.events.length === 0 ? (
            <p className="hint">The machine has recorded no events yet.</p>
          ) : (
            <ol className="self-events">
              {self.events.map((event, index) => (
                <li key={`${event.id ?? index}-${event.event}`} className="self-event">
                  <span className="self-when mono">
                    {event.tsMs === null ? "—" : clock(event.tsMs)}
                  </span>
                  <span className="self-what mono">
                    {event.event}
                    {event.tool !== null ? ` ${event.tool}` : ""}
                  </span>
                  <span className="self-meta">
                    {event.decision !== null ? `${event.decision} · ` : ""}
                    {event.transport !== null ? `${event.transport}` : ""}
                    {event.ok !== null ? (event.ok ? " · ok" : " · failed") : ""}
                  </span>
                </li>
              ))}
            </ol>
          )}
        </>
      )}

    </details>
  );
}

/** The device's stamps are Unix MILLISECONDS; the page prints a local clock with seconds. */
function clock(ms: number): string {
  const at = new Date(ms);
  if (Number.isNaN(at.getTime())) return "—";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}`;
}
