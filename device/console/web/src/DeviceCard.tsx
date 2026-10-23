export type Device = {
  id: string;
  name: string | null;
  online: boolean;
  version: string | null;
  last_seen_ms: number | null;
  last_attempt?: { attempts: number; reason: string } | null;
};

type Props = {
  device: Device;
  newest: string | null;
  selected: boolean;
  onSelect: () => void;
  onRename: () => void;
  onRemove: () => void;
};

export function DeviceCard({ device, newest, selected, onSelect, onRename, onRemove }: Props) {
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

      {device.last_attempt && device.last_attempt.attempts > 0 && (
        // A device cannot report a failure on the connection that failed, so this is what it said the
        // NEXT time it got through — the label is honest about being one attempt behind. Shown online
        // as well, because "it flapped twice and came back" is the fact an operator wants before the
        // next outage, not after it.
        <p className="attempt" title={device.last_attempt.reason}>
          {device.online ? "reconnected after " : "last connect: "}
          {device.last_attempt.attempts} failed attempt
          {device.last_attempt.attempts === 1 ? "" : "s"}
          {device.last_attempt.reason ? ` — ${device.last_attempt.reason}` : ""}
        </p>
      )}

      <p className="mono id">{device.id}</p>

      <footer>
        <button type="button" className="primary" onClick={onSelect}>Work on this</button>
        <button type="button" onClick={onRename}>Rename</button>
        <button type="button" className="danger" onClick={onRemove}>Remove</button>
      </footer>
    </article>
  );
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
