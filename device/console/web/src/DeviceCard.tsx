import { useState } from "react";
import type { HostFacts, UpdateGeneration, UpdateRepair, UpdateSwap } from "./api";
import { meshReadiness, type MeshReadiness } from "./api";
import { shortId } from "./format";
import { VitalsChart } from "./VitalsChart";
import type { VitalsSample } from "./vitals";

/** The device reports at most this many repairs, so a full list means "at least this many". */
const REPAIR_LIMIT = 5;

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
  /** This browser's sampled history for the device; empty until the second sample exists. */
  vitals?: VitalsSample[];
  onSelect: () => void;
  onRename: () => void;
  onRemove: () => void;
  onDiagnostics: () => void;
  onConnect: () => void;
};

export function DeviceCard({ device, newest, selected, vitals = [], onSelect, onRename, onRemove, onDiagnostics, onConnect }: Props) {
  const behind = device.version !== null && newest !== null && device.version !== newest;
  // A readiness check is on demand: the page must not ask the broker about every device on every load.
  const [mesh, setMesh] = useState<MeshReadiness | null>(null);
  const [checking, setChecking] = useState(false);
  const meshText = (() => {
    if (!mesh) return "not checked";
    if ("error" in mesh) return mesh.error;
    if (!mesh.configured) return "console has no broker";
    return mesh.reachable
      ? `reachable (${mesh.candidates} candidate${mesh.candidates === 1 ? "" : "s"})`
      : "not registered";
  })();
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
        <div>
          <dt>Mesh</dt>
          <dd className={mesh && "reachable" in mesh && mesh.reachable ? "ok" : "muted"}>{meshText}</dd>
        </div>
      </dl>

      {device.host && <HostFactsBlock host={device.host} vitals={vitals} />}

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
        <button
          type="button"
          disabled={checking}
          onClick={async () => {
            setChecking(true);
            setMesh(await meshReadiness(device.id));
            setChecking(false);
          }}
        >
          {checking ? "Checking…" : "Check mesh"}
        </button>
        <button type="button" onClick={onConnect} title="Hand an AI client an MCP endpoint and a grant">
          AI client
        </button>
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
 *
 * The update row is what the launcher wrote into its baseline directories. A staged pair (`pending`)
 * is not yet proven — it is written before the probe, and the launcher promotes it only after the new
 * build answers, discarding it otherwise — so the row says "staged" and never "updating" or
 * "restarted": the swap is the launcher's business, and a pending pair is the fact the launcher has
 * left on disk. Three nulls (a device with no baselines) and an absent `update` key (a device too old
 * to report one) both draw no row at all.
 *
 * The two rows below it answer the other half of "it came back": the launcher's last swap result and
 * its restores. A refused swap was declined before anything was replaced, so it says "refused" and
 * never "applied" or "updated"; a rollback names the version it left; an applied swap is dated by
 * how long ago it happened. The repairs row counts them and names the generation only when it was
 * not the baseline. A record the launcher did not write is absent, and an absent record draws no row.
 *
 * The vitals chart is not a device fact: it is this browser's own sample of the two rows above it,
 * drawn after them because the live numbers are the current truth and the line is the history.
 */
function HostFactsBlock({ host, vitals }: { host: HostFacts; vitals: VitalsSample[] }) {
  const memory = host.memory;
  const sessions = host.sessions ?? [];
  const update = updateSentence(host.update);
  const lastSwap = swapSentence(host.update?.last_swap);
  const repairs = host.update?.repairs ?? [];
  const system = [host.os?.name, host.os?.version].filter(Boolean).join(" ") || "unknown system";
  return (
    <>
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
        {update && (
          <div>
            <dt>Update</dt>
            <dd
              title={
                host.update?.pending
                  ? "Not yet proven: the launcher promotes a staged pair only after the new build answers, and discards it otherwise."
                  : undefined
              }
            >
              {update}
            </dd>
          </div>
        )}
        {lastSwap && (
          <div>
            <dt>Last swap</dt>
            <dd>{lastSwap}</dd>
          </div>
        )}
        {repairs.length > 0 && (
          <div>
            <dt>Repaired</dt>
            <dd title={repairsTitle(repairs)}>{repairsSentence(repairs)}</dd>
          </div>
        )}
        <div>
          <dt>Helper</dt>
          <dd className={host.helper?.registered ? undefined : "muted"}>
            {host.helper?.registered ? `session ${host.helper.session ?? "?"}` : "not running"}
          </dd>
        </div>
      </dl>
      <VitalsChart samples={vitals} />
    </>
  );
}

function sessionSentence(session: { id: number; user?: string | null; state: string }): string {
  const who = session.user ? `${session.user}@${session.id}` : `#${session.id}`;
  return `${who} ${session.state}`;
}

/**
 * The launcher's three generations, in one line, omitting the ones that are not there.
 *
 * `pending` is called "staged" and carries "not yet proven" because that is all it is: the launcher
 * wrote the pair before the probe and promotes it only after the new build answers, discarding it
 * otherwise. Every other word for it ("updating", "pending restart") would claim a swap this row
 * cannot see. A generation with no version is left out whole rather than shown as a blank, and three
 * empty generations return "", so the caller draws no row — which is what every device without
 * baselines reports.
 *
 * Only the staged pair shows its channel: that is what tells an operator where an update in flight
 * came from, and three channels on one line would bury the versions.
 */
function updateSentence(update: HostFacts["update"]): string {
  const pending = update?.pending;
  const staged = versionLabel("staged", pending);
  const parts: (string | null)[] = [
    staged ? `${staged}${pending?.channel ? ` (${pending.channel})` : ""}, not yet proven` : null,
    versionLabel("current", update?.current),
    versionLabel("previous", update?.previous)
  ];
  return parts.filter((part): part is string => part !== null).join(" · ");
}

/** `current 0.3.29`, or null when the generation reported no version. */
function versionLabel(label: string, generation: UpdateGeneration | null | undefined): string | null {
  return generation?.version ? `${label} ${generation.version}` : null;
}

/**
 * The launcher's last swap in one line, or null when there is no result to show.
 *
 * A refusal is not an update: the launcher declined it before anything on disk was replaced, so it
 * says "refused" and never "applied" or "updated". A rollback names the version it returned from
 * and the reason when the launcher kept one; an applied swap names the version that took over and
 * how long ago that was. An outcome this console does not know returns null, so a result from a
 * newer launcher draws no line rather than a wrong one.
 */
function swapSentence(swap: UpdateSwap | null | undefined): string | null {
  if (!swap) return null;
  switch (swap.outcome) {
    case "applied":
      return `applied${swap.version ? ` ${swap.version}` : ""} · ${age(swap.at_ms)}`;
    case "rolled_back":
      return `rolled back${swap.version ? ` from ${swap.version}` : ""}${swap.reason ? `: ${swap.reason}` : ""}`;
    case "refused":
      return `refused${swap.reason ? `: ${swap.reason}` : ""}`;
    default:
      return null;
  }
}

/**
 * How often the launcher restored the app, and from where, in one line.
 *
 * The row's label supplies "Repaired", so the value starts with the count ("twice", "4 times").
 * The count is what the row is for, and it is honest about the device's cap: only the newest five
 * repairs are reported, so five records may be a longer history cut off, and that row says "at
 * least 5 times"; an exact count is claimed only below the cap.
 *
 * The `from` values are named only when at least one restore came from `previous`: a repair from
 * the baseline is the normal one, and naming it every time would bury the case that answers "the
 * current generation itself was unusable". The launcher writes only those two values; anything
 * else is not counted as a generation this row knows, and an empty list draws no row.
 */
function repairsSentence(repairs: UpdateRepair[]): string | null {
  if (repairs.length === 0) return null;
  const count = repairs.length;
  const previous = repairs.filter((repair) => repair.from === "previous").length;
  const total = `${count === REPAIR_LIMIT ? "at least " : ""}${times(count)}`;
  if (previous === 0) return total;
  if (previous === count) {
    if (count === 1) return "once from the previous generation";
    return `${total}, ${count === 2 ? "both" : "all"} from the previous generation`;
  }
  const fromPrevious = previous === 1 ? "once" : previous === 2 ? "twice" : `${previous} of them`;
  return `${total}, ${fromPrevious} from the previous generation`;
}

/** "once", "twice", or "3 times": a repair count as the row says it. */
function times(count: number): string {
  return count === 1 ? "once" : count === 2 ? "twice" : `${count} times`;
}

/** The repairs row's hover: each record's time, generation and hash, newest first. */
function repairsTitle(repairs: UpdateRepair[]): string {
  return repairs
    .map((repair) => `${repairTime(repair.at_ms)} ${repair.from} ${repair.sha256}`)
    .join("\n");
}

/**
 * A repair's ISO time, or its raw milliseconds when the value is outside `Date`'s range.
 *
 * `at_ms` is a `u64` on the wire, so a corrupted-but-parseable log line can carry a value no
 * `Date` can hold; formatting it must not throw in render and blank the whole card.
 */
function repairTime(at_ms: number): string {
  const date = new Date(at_ms);
  return Number.isNaN(date.getTime()) ? `${at_ms} ms` : date.toISOString();
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

function age(ms: number | null): string {
  if (ms === null) return "never";
  const seconds = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  return `${Math.round(minutes / 60)}h ago`;
}
