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
  newest: string | null;
  selected: boolean;
  onSelect: () => void;
  onRename: () => void;
  onRemove: () => void;
  onDiagnostics: () => void;
  onConnect: () => void;
};

export function DeviceCard({ device, newest, selected, onSelect, onRename, onRemove, onDiagnostics, onConnect }: Props) {
  const behind = device.version !== null && newest !== null && device.version !== newest;
  return (
    <article className={"card" + (selected ? " card-selected" : "")}>
      <header>
        <span className={"dot " + (device.online ? "dot-ok" : "dot-off")} />
        <strong>{device.name ?? shortId(device.id)}</strong>
        {behind && device.version && (
          <span className="chip chip-warn" title={`Another device in this list runs ${newest}.`}>
            v{device.version}
          </span>
        )}
      </header>

      {/* THE CARD ANSWERS THE PICKER'S QUESTION, NOT THE MACHINE PAGE'S. Name, reachability,
          build and one machine line are what an operator chooses between; memory, disks, sessions,
          swaps, repairs, the helper, the raw id and the vitals history live under Details, which is
          a disclosure the operator opens deliberately. Before this, every card carried the whole
          machine page and the picker read as a wall of diagnostics. */}
      <p className="card-status">
        <span className={device.online ? "ok" : "muted"}>{device.online ? "online" : "offline"}</span>
        <span aria-hidden="true">·</span>
        <span className="muted">seen {age(device.last_seen_ms)}</span>
        {device.version && !behind && (
          <>
            <span aria-hidden="true">·</span>
            <span className="muted">v{device.version}</span>
          </>
        )}
        {device.host && (
          <>
            <span aria-hidden="true">·</span>
            <span className="muted">{machineLine(device.host)}</span>
          </>
        )}
      </p>

      {device.last_attempt && device.last_attempt.attempts > 0 && (
        // A device cannot report a failure on the connection that failed, so this is what it said the
        // NEXT time it got through — the label is honest about being one attempt behind. Shown online
        // as well, because "it flapped twice and came back" is the fact an operator wants before the
        // next outage, not after it.
        <p className="attempt" title={device.last_attempt.reason}>
          {retrySentence(device.last_attempt.attempts, device.last_attempt.reason)}
        </p>
      )}

      <footer>
        <button type="button" className="primary" onClick={onSelect}>Work on this</button>
        <button type="button" onClick={onConnect} title="Hand an AI client an MCP endpoint and a token">
          Connect AI client
        </button>
        <details className="card-details">
          <summary>Details</summary>
          <div className="card-details-body">
            <p className="hint">
              Memory, disks, sessions and update state are on the Machine page. This device's id and
              the console link are in Copy diagnostics.
            </p>
            <div className="card-details-actions">
              <button type="button" onClick={onDiagnostics}>Copy diagnostics</button>
              <button type="button" onClick={onRename}>Rename</button>
              <button type="button" className="danger" onClick={onRemove}>Remove</button>
            </div>
          </div>
        </details>
      </footer>
    </article>
  );
}

/** The one machine line a picker needs: what it is and how long it has been up. */
function machineLine(host: HostFacts): string {
  const system = [host.os?.name, host.os?.version].filter(Boolean).join(" ") || "system unknown";
  return host.uptime_s ? `${system} · up ${duration(host.uptime_s)}` : system;
}

function duration(seconds?: number): string {
  if (!seconds) return "unknown";
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  if (days > 0) return `${days}d ${hours}h`;
  const minutes = Math.floor((seconds % 3600) / 60);
  return hours > 0 ? `${hours}h ${minutes}m` : `${minutes}m`;
}

/**
 * The one sentence both surfaces use for a link that came back.
 *
 * The panel says it too — same words, same order — because an operator reading one surface and then the
 * other should not have to translate. "offline" alone is not a diagnosis; this is.
 */
export function retrySentence(attempts: number, reason: string): string {
  const noun = attempts === 1 ? "failed attempt" : "failed attempts";
  const base = `reconnected after ${attempts} ${noun}`;
  return reason ? `${base} — ${reason}` : base;
}
