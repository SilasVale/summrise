import type { DeviceUpdate, HostFacts } from "./api";
import type { LastAttempt } from "./attempt";
import { callsLine, lastHeardMs, seenAgo, shortId } from "./format";
import { startedLine, processStartMs } from "./updateStory";

export type Device = {
  id: string;
  name: string | null;
  online: boolean;
  version: string | null;
  /** When the console last saw a frame, or absent when it has seen none since it started. */
  last_frame_ms?: number | null;
  /** When the current silence started, from the newest still-open `device.left`. */
  last_left_ms?: number | null;
  /** The registry's last channel open: older evidence than either of the two above. */
  last_seen_ms?: number | null;
  /**
   * The AI's calls the local calendar day holds for this machine, and how many failed. Absent when
   * the day names the machine nowhere: the row then draws no day at all, never a zero. `since_ms`
   * and `bounded` say where a partial day's coverage starts rather than letting it read as whole.
   */
  today?:
    | {
        client_calls: number;
        failed: number;
        since_ms?: number | null;
        bounded?: boolean;
        scan_limit?: number | null;
      }
    | null;
  /**
   * The attempt before the current channel. The row does not draw it: it is a count and a reason
   * with no moment of its own, and a sentence about it read as today's fault (C9). The field stays
   * because the page passes the fleet's row unchanged, and a test pins that the row ignores it.
   */
  last_attempt?: LastAttempt | null;
  host?: HostFacts | null;
  /** The device's own update story, absent when it never sent one. */
  update?: DeviceUpdate | null;
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
  // **THE AGE IS THE NEWEST EVIDENCE THE CONSOLE HAS.** A frame proves presence and wins; a console
  // that restarted mid-outage still has the audit's newest open `device.left`; the registry's
  // `last_seen_ms` is the last channel open, older than either. The row states the same moment the
  // notice will, and `seenAgo` keeps the minutes so "1h 49m" cannot read as "2h".
  const heard = lastHeardMs(device);
  const state =
    heard === null
      ? device.online
        ? "online"
        : "offline"
      : `${device.online ? "online" : "offline"} · ${
          device.online ? "seen" : "last seen"
        } ${seenAgo(heard)}`;
  // NO RECONNECT SENTENCE. `last_attempt` is a count and a reason with no moment of its own: both
  // online rows said "Reconnected after 1 attempt — …" at 10:07 about the 09:41/09:43 restarts, and
  // the sentence read as a fault that was happening now. `/fleet` carries no time for the attempt,
  // and the last frame is after the reconnect rather than at it, so any date here would be borrowed
  // from a different fact. The row draws neither the count nor the reason; Copy diagnostics keeps
  // the attempt as a record and dates the moment the console last heard the machine.
  // The device's own account of its last start, beside the console's account of its silence. The two
  // lines are independent on purpose: the console knows frames stopped, not that the machine slept,
  // and the device knows what it started, not when the console last heard it. The version is the
  // row's chip above, so the device's line never repeats it — it draws only an update that belongs
  // to this start (aged) or the device's own current sentence.
  const started = startedLine(device.update, {
    startedAtMs: processStartMs(device),
    shownVersion: device.version
  });
  // **THE ROW COMPARES THE WORK, NOT ONLY THE PRESENCE.** The day is the fleet read's own count
  // from the local calendar day the hub counts: no request, no page, and nothing for a machine the
  // day never names. When the audit file starts after midnight the line says `since`, so a partial
  // day's 554 calls cannot read as the day's whole 649. It shares the state's muted line so the row
  // stays one row at 1440; at 390 the status cell takes its own line and the counts wrap below the
  // state.
  const work = callsLine(device.today);
  return (
    <article className={"fleet-row" + (selected ? " is-current" : "")}>
      <button type="button" className="fleet-open" aria-label={`Open ${label}`} onClick={onSelect}>
        <span className={"dot " + (device.online ? "dot-ok" : "dot-off")} aria-hidden="true" />
        <span className="fleet-name">{label}</span>
        <span className="fleet-now">
          <span className={"fleet-state" + (device.online ? " ok" : "")}>{state}</span>
          {work && <span className="fleet-work">{work}</span>}
        </span>
        {device.version && <span className="fleet-build mono">v{device.version}</span>}
        <span className="fleet-chevron" aria-hidden="true">›</span>
        {started && (
          <span className="fleet-verdict" title={started}>
            {started}
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
