import type { HostFacts } from "./api";
import { AttemptReason, attemptCount, attemptReason, type LastAttempt } from "./attempt";
import { age, shortId } from "./format";

export type Device = {
  id: string;
  name: string | null;
  online: boolean;
  version: string | null;
  /** When the console last saw a frame, or absent when it has seen none since it started. */
  last_frame_ms?: number | null;
  last_attempt?: LastAttempt | null;
  host?: HostFacts | null;
};

type Props = {
  device: Device;
  selected: boolean;
  onSelect: () => void;
  onRename: () => void;
  onRemove: () => void;
  onDiagnostics: () => void;
};

/**
 * One machine on the fleet. The row is the target: the main button opens the machine, and the
 * overflow holds the three management actions. There is no per-row Connect and no accent on a row:
 * the screen's one primary action is Add a machine.
 */
export function DeviceCard({ device, selected, onSelect, onRename, onRemove, onDiagnostics }: Props) {
  const label = device.name ?? shortId(device.id);
  // **THE AGE IS THE LAST FRAME.** `online` is that same moment inside the console's presence
  // window, so the row states one fact instead of two: a socket that exists but has gone silent is
  // offline, and the age says when the console last heard it rather than when it last connected.
  const heard = device.last_frame_ms ?? null;
  const state =
    heard === null
      ? device.online
        ? "online"
        : "offline"
      : `${device.online ? "online" : "offline"} · ${
          device.online ? "seen" : "last seen"
        } ${age(heard)}`;
  // The count and the device's own reason on one muted line. The reason is the diagnosis: a proxy's
  // 502 and a channel the console dropped are different faults, and the row must not read them as
  // the same healthy reconnect. A device that has not reconnected says nothing — and a silent
  // channel is not a reconnect, so an open-then-quiet device gets no attempt line at all.
  const attempt = device.online ? device.last_attempt : null;
  const count = attempt && attempt.attempts > 0 ? attemptCount(attempt.attempts) : null;
  const reason = attemptReason(attempt);
  const sentence = count ? `Reconnected after ${count}${reason ? ` — ${reason}` : ""}` : null;
  return (
    <article className={"fleet-row" + (selected ? " is-current" : "")}>
      <button type="button" className="fleet-open" aria-label={`Open ${label}`} onClick={onSelect}>
        <span className={"dot " + (device.online ? "dot-ok" : "dot-off")} aria-hidden="true" />
        <span className="fleet-name">{label}</span>
        <span className={"fleet-state" + (device.online ? " ok" : "")}>
          {state}
        </span>
        {device.version && <span className="fleet-build mono">v{device.version}</span>}
        <span className="fleet-chevron" aria-hidden="true">›</span>
        {count && (
          <span className="fleet-attempt" title={reason ? (sentence ?? undefined) : undefined}>
            Reconnected after {count}
            {reason && (
              <>
                {" — "}
                <AttemptReason reason={reason} />
              </>
            )}
          </span>
        )}
      </button>
      <details className="row-menu">
        <summary aria-label={`More about ${label}`} title={`More about ${label}`}>⋯</summary>
        <div className="row-menu-pop">
          <button type="button" onClick={onDiagnostics}>
            Copy diagnostics
          </button>
          <button type="button" onClick={onRename}>
            Rename
          </button>
          <button type="button" className="danger" onClick={onRemove}>
            Remove
          </button>
        </div>
      </details>
    </article>
  );
}
