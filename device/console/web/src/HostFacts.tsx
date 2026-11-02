import type { HostFacts, UpdateGeneration, UpdateRepair, UpdateSwap } from "./api";
import { age } from "./format";
import { VitalsChart } from "./VitalsChart";
import type { VitalsSample } from "./vitals";

/** The device reports at most this many repairs, so a full list means "at least this many". */
const REPAIR_LIMIT = 5;

/**
 * What the machine is, as the device last reported it — THE MACHINE PAGE'S SUBJECT.
 *
 * It used to live inside every card on the Devices picker, which made the picker a wall of diagnostics
 * and left the machine's own facts with no home once a device was selected. The card now carries the
 * decision (name, reachability, build, one machine line); this block sits on Machine, where the whole
 * question is "what is this thing and how is it doing".
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
 * never "applied" or "updated"; a rollback names the version it left; an applied swap is dated by how
 * long ago it happened. The repairs row counts them and names the generation only when it was not the
 * baseline. A record the launcher did not write is absent, and an absent record draws no row.
 *
 * The vitals chart is not a device fact: it is this browser's own sample of the two rows above it,
 * drawn after them because the live numbers are the current truth and the line is the history.
 */
export function HostFactsBlock({ host, vitals }: { host: HostFacts; vitals: VitalsSample[] }) {
  const memory = host.memory;
  const sessions = host.sessions ?? [];
  const update = updateSentence(host.update);
  const lastSwap = swapSentence(host.update?.last_swap);
  const repairs = host.update?.repairs ?? [];
  return (
    <>
      <dl className="facts host">
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
 * says "refused" and never "applied" or "updated". A rollback names the version it returned from and
 * the reason when the launcher kept one; an applied swap names the version that took over and how
 * long ago that was. An outcome this console does not know returns null, so a result from a newer
 * launcher draws no line rather than a wrong one.
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
 * The row's label supplies "Repaired", so the value starts with the count ("twice", "4 times"). The
 * count is what the row is for, and it is honest about the device's cap: only the newest five repairs
 * are reported, so five records may be a longer history cut off, and that row says "at least 5
 * times"; an exact count is claimed only below the cap.
 *
 * The `from` values are named only when at least one restore came from `previous`: a repair from the
 * baseline is the normal one, and naming it every time would bury the case that answers "the current
 * generation itself was unusable". The launcher writes only those two values; anything else is not
 * counted as a generation this row knows, and an empty list draws no row.
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
 * `at_ms` is a `u64` on the wire, so a corrupted-but-parseable log line can carry a value no `Date`
 * can hold; formatting it must not throw in render and blank the whole page.
 */
function repairTime(at_ms: number): string {
  const date = new Date(at_ms);
  return Number.isNaN(date.getTime()) ? `${at_ms} ms` : date.toISOString();
}

function gib(bytes?: number): string {
  if (!bytes) return "0 GiB";
  return `${(bytes / 1024 ** 3).toFixed(1)} GiB`;
}
