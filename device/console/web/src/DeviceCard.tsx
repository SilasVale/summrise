import type { HostFacts } from "./api";

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
};

export function DeviceCard({ device, newest, selected, onSelect, onRename, onRemove, onDiagnostics }: Props) {
  const behind = device.version !== null && newest !== null && device.version !== newest;
  return (
    <article className={"card" + (selected ? " card-selected" : "")}>
      <header>
        <span className={"dot " + (device.online ? "dot-ok" : "dot-off")} />
        <strong>{device.name ?? shortId(device.id)}</strong>
        {device.version && (
          <span className={"chip " + (behind ? "chip-warn" : "chip-ok")} title={behind ? `Another device in this list runs ${newest}.` : undefined}>
            v{device.version}
          </span>
        )}
      </header>

      <dl className="facts">
        <div>
          <dt>Channel</dt>
          <dd className={device.online ? "ok" : "muted"}>{device.online ? "online" : "offline"}</dd>
        </div>
        <div>
          <dt>Last seen</dt>
          <dd>{age(device.last_seen_ms)}</dd>
        </div>
      </dl>

      {device.host && <HostFactsBlock host={device.host} />}

      {device.last_attempt && device.last_attempt.attempts > 0 && (
        // A device cannot report a failure on the connection that failed, so this is what it said the
        // NEXT time it got through — the label is honest about being one attempt behind. Shown online
        // as well, because "it flapped twice and came back" is the fact an operator wants before the
        // next outage, not after it.
        <p className="attempt" title={device.last_attempt.reason}>
          {retrySentence(device.last_attempt.attempts, device.last_attempt.reason)}
        </p>
      )}

      <p className="mono id">{device.id}</p>

      <footer>
        <button type="button" className="primary" onClick={onSelect}>Work on this</button>
        <button type="button" onClick={onDiagnostics}>Diagnostics</button>
        <button type="button" onClick={onRename}>Rename</button>
        <button type="button" className="danger" onClick={onRemove}>Remove</button>
      </footer>
    </article>
  );
}

/**
 * What the machine is, as the device last reported it.
 *
 * The session row is the one that resolves the question a capture tool otherwise raises by surprise:
 * a disconnected or idle session is why a desktop has no picture, and it is a fact about the host, not
 * an error to discover.
 */
function HostFactsBlock({ host }: { host: HostFacts }) {
  const memory = host.memory;
  const sessions = host.sessions ?? [];
  const system = [host.os?.name, host.os?.version].filter(Boolean).join(" ") || "unknown system";
  return (
    <dl className="facts host">
      <div>
        <dt>Machine</dt>
        <dd>{system} · up {duration(host.uptime_s)}</dd>
      </div>
      {memory && memory.total_bytes ? (
        <div>
          <dt>Memory</dt>
          <dd>{gib(memory.available_bytes)} free of {gib(memory.total_bytes)}</dd>
        </div>
      ) : null}
      {(host.disks ?? []).map((disk) => (
        <div key={disk.name}>
          <dt>{disk.name} free</dt>
          <dd>{gib(disk.free_bytes)} of {gib(disk.total_bytes)}</dd>
        </div>
      ))}
      {sessions.length > 0 && (
        <div>
          <dt>Sessions</dt>
          <dd>{sessions.map(sessionSentence).join(" · ")}</dd>
        </div>
      )}
      <div>
        <dt>Helper</dt>
        <dd className={host.helper?.registered ? undefined : "muted"}>
          {host.helper?.registered ? `session ${host.helper.session ?? "?"}` : "not running"}
        </dd>
      </div>
    </dl>
  );
}

function sessionSentence(session: { id: number; user?: string | null; state: string }): string {
  const who = session.user ? `${session.user}@${session.id}` : `#${session.id}`;
  return `${who} ${session.state}`;
}

function gib(bytes?: number): string {
  if (!bytes) return "0 GiB";
  return `${(bytes / 1024 ** 3).toFixed(1)} GiB`;
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

function shortId(id: string): string {
  return id.slice(0, 12) + "…";
}

function age(ms: number | null): string {
  if (ms === null) return "never";
  const seconds = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  return `${Math.round(minutes / 60)}h ago`;
}
