import type { HostFacts } from "./api";
import { age, shortId } from "./format";

export type Device = {
  id: string;
  name: string | null;
  online: boolean;
  version: string | null;
  last_seen_ms: number | null;
  last_attempt?: { attempts: number; reason: string } | null;
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
  const attempt =
    device.online && device.last_attempt && device.last_attempt.attempts > 0
      ? `Reconnected after ${device.last_attempt.attempts} attempt${device.last_attempt.attempts === 1 ? "" : "s"}`
      : null;
  return (
    <article className={"fleet-row" + (selected ? " is-current" : "")}>
      <button type="button" className="fleet-open" aria-label={`Open ${label}`} onClick={onSelect}>
        <span className={"dot " + (device.online ? "dot-ok" : "dot-off")} aria-hidden="true" />
        <span className="fleet-name">{label}</span>
        <span className={"fleet-state" + (device.online ? " ok" : "")}>
          {device.online ? "online" : "offline"} ·{" "}
          {device.online ? `seen ${age(device.last_seen_ms)}` : `last seen ${age(device.last_seen_ms)}`}
        </span>
        {device.version && <span className="fleet-build mono">v{device.version}</span>}
        <span className="fleet-chevron" aria-hidden="true">›</span>
        {attempt && <span className="fleet-attempt">{attempt}</span>}
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
