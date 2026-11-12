import type { HostFacts, UpdateGeneration, UpdateRepair, UpdateSwap } from "./api";
import { age, clockTime, exactMoment, isoDate, seenAgo } from "./format";
import { VitalsChart } from "./VitalsChart";
import type { VitalsSample } from "./vitals";

/** The device reports at most this many repairs, so a full list means "at least this many". */
const REPAIR_LIMIT = 5;

/** The repair task's definition, carried by the row whether or not its facts arrived. */
const REPAIR_TASK_TITLE =
  "The five-minute task that starts SummriseLaunch when it is stopped and re-registers it when it is missing. The values are the machine's Task Scheduler answer, not a console guess.";

/**
 * What an absent repair-task fact means, in the hover.
 *
 * The visible words can only say the frame carried no fact. Why it carried none is not something
 * the console can prove: a build older than the fact sends nothing, and a current build whose Task
 * Scheduler did not answer sends nothing either. So the hover names both and claims neither that
 * the task is healthy nor that it is missing.
 */
const REPAIR_TASK_ABSENT_TITLE =
  "The machine's host frame carried no repair task fact. A build older than the fact sends none, and a Task Scheduler that did not answer sends none either; the console cannot tell which, so this says neither healthy nor missing.";

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
 * The repair task row is the machine's own Task Scheduler answer, and its absence is not one of
 * those. A build older than the fact sends no `repair_task`, so an online machine whose frame lacks
 * it draws the row with the page's absent words and the running version: a silent gap reads as
 * "there is no repair task" or "it is healthy", and neither is what the console knows. An offline
 * block keeps the dated-absence shape and draws no row: only a live machine's missing fact is a
 * statement about the build it runs.
 *
 * The vitals chart is not a device fact: it is this browser's own sample of the two rows above it,
 * drawn after them because the live numbers are the current truth and the line is the history.
 */
/**
 * What the machine is, in the frame's own values: `Windows 11 · x86_64 · 12 cores`.
 *
 * `release` is the human name the device reports (`Windows 11`) and wins over `version`
 * (`10.0.26100`); a frame that carries only name and version still says both, so an older device
 * is not left with a bare `Windows`. Architecture and core count join with the same separator, and
 * a fact the frame did not carry is omitted rather than printed as `undefined` or `0 cores`.
 */
export function platformLine(host: HostFacts): string | null {
  const name =
    host.os?.release?.trim() ||
    [host.os?.name?.trim(), host.os?.version?.trim()].filter(Boolean).join(" ");
  const cores = host.cpu?.cores;
  const parts = [
    name || null,
    host.os?.arch?.trim() || null,
    typeof cores === "number" && cores > 0 ? `${cores} ${cores === 1 ? "core" : "cores"}` : null
  ];
  return parts.filter((part): part is string => part !== null).join(" · ") || null;
}

export function HostFactsBlock({
  host,
  vitals,
  collecting = true,
  readAtMs = null,
  readFrom = "frame",
  online = false,
  runningVersion = null
}: {
  host: HostFacts;
  vitals: VitalsSample[];
  /** False when the machine is offline: the vitals caption must not promise sampling. */
  collecting?: boolean;
  /**
   * When the console last saw a frame from this machine, for a fact block that is not current.
   *
   * **THE FACTS ARE THE FRAME'S, AND THE FRAME HAS AN AGE.** The measured 9VH had been offline five
   * hours, and `#3 connected` / `Helper not registered this boot` still read as this moment's facts.
   * Dating the block is what makes the tense honest: the values stay exactly as the device reported
   * them, and the sentence says when that was. `null` draws no date (a device that has sent no frame
   * has no moment to name), and a live block is not dated at all.
   */
  readAtMs?: number | null;
  /**
   * Which evidence `readAtMs` is: the frame the values came from, or — when a console restarted
   * mid-outage and has no frame date — the registry's last-seen moment. The prefix must name the
   * source, because "last frame 5h ago" and "last seen 5h ago" are different claims about the facts.
   */
  readFrom?: "frame" | "seen" | "read";
  /**
   * The fleet's reachability for this machine. Only a live machine's absent repair-task fact draws
   * the "not reported by this build" row: an offline block's facts are its last frame and already
   * dated, so the row would name a running build whose facts the console cannot read.
   */
  online?: boolean;
  /**
   * The version the fleet says the machine runs, named by the repair task row when the frame carries
   * no fact for it. Null draws the sentence without a version rather than inventing one.
   */
  runningVersion?: string | null;
}) {
  const memory = host.memory;
  const sessions = host.sessions ?? [];
  const update = updateSentence(host.update);
  const lastSwap = swapSentence(host.update?.last_swap);
  const repairs = host.update?.repairs ?? [];
  const repairTask = host.repair_task;
  return (
    <>
      {readAtMs !== null && (
        // THE BLOCK'S OWN CLOCK. Not "now": the age is the distance from the moment above, and the
        // exact instant stays in the title for anyone matching it to a log. A last-seen date is not
        // a frame date, so the sentence says what it is rather than handing the values that moment.
        <p className="hint facts-read" title={exactMoment(readAtMs)}>
          {readFrom === "read"
            ? `Read from the machine just now.`
            : readFrom === "frame"
              ? `Last frame ${seenAgo(readAtMs)}. These facts are the machine's last frame, not a live read.`
              : `Last seen ${seenAgo(readAtMs)}. These facts are the machine's last frame; the console has no timestamp for that frame.`}
        </p>
      )}
      <dl className="facts host">
        {platformLine(host) !== null && (
          // **WHAT THE MACHINE IS, BEFORE WHAT IT HAS (audit-44 F8).** The frame carries the OS,
          // its architecture and its core count; the page used to start at Memory and never name
          // any of them. One line above Memory, using the frame's own values and omitting a fact
          // the frame did not carry rather than printing a zero.
          <div>
            <dt>Platform</dt>
            <dd>{platformLine(host)}</dd>
          </div>
        )}
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
        {(repairTask || online) && (
          <div>
            <dt title={REPAIR_TASK_TITLE}>
              Repair task
            </dt>
            {repairTask ? (
              <dd
                className={repairTask.registered ? undefined : "muted"}
                title={repairTaskTitle(repairTask)}
              >
                {repairTaskSentence(repairTask)}
              </dd>
            ) : (
              // **AN ONLINE MACHINE'S SILENT FACT IS NOT "NO TASK".** The measured 84T on 0.3.291 has
              // no `repair_task` key: the row used to vanish, which read as a machine with no repair
              // path. The row now states what the frame shows and names the build whose frame it is;
              // `not registered` stays the device's own `registered: false`, never this absence.
              <dd className="muted" title={REPAIR_TASK_ABSENT_TITLE}>
                {absentRepairTaskSentence(runningVersion)}
              </dd>
            )}
          </div>
        )}
        <div>
          <dt
            title={
              "The per-user desktop helper: it takes screenshots and runs commands in the signed-in Windows session, which the device service cannot see. The Screen page needs it; nothing else does."
            }
          >
            Helper
          </dt>
          <dd className={host.helper?.registered ? undefined : "muted"}>
            {helperSentence(host.helper)}
          </dd>
        </div>
      </dl>
      <VitalsChart samples={vitals} collecting={collecting} />
    </>
  );
}

/**
 * The repair task's value, in the machine's own Task Scheduler facts.
 *
 * `not registered` is a fact the machine reported (the task is absent), not a missing answer: the
 * missing answer is `absentRepairTaskSentence` below. `registered` always leads when the task
 * exists; the last run and its result are stated only when Task Scheduler reported them, and `next`
 * only when the task has a next run. **A ZERO RESULT IS THE SCHEDULER'S SUCCESS, NOT A CODE TO
 * DECODE** (audit-44 F11): it reads `last result 0 (success)`, while Task Scheduler's failures stay
 * as the audit writes them (`0x80070002`) because that is the number an operator matches to a log.
 * The raw code stays in the row's title either way.
 */
function repairTaskSentence(task: NonNullable<HostFacts["repair_task"]>): string {
  if (!task.registered) return "not registered";
  const parts = ["registered"];
  if (task.last_run_ms != null) parts.push(`last run ${taskMoment(task.last_run_ms)}`);
  if (task.last_result != null) {
    parts.push(
      `last result ${taskResult(task.last_result)}${task.last_result === 0 ? " (success)" : ""}`
    );
  }
  if (task.next_run_ms != null) parts.push(`next ${taskMoment(task.next_run_ms)}`);
  return parts.join(" · ");
}

/**
 * The repair result's raw code, for the row's hover.
 *
 * `0 (success)` is a verdict, and a verdict can lose the evidence it came from: the Task Scheduler
 * number is what a log repeats, so it stays here unaltered — decimal, and the hexadecimal spelling
 * for a failure, the same pair the visible row uses.
 */
function repairTaskTitle(
  task: NonNullable<HostFacts["repair_task"]>
): string | undefined {
  if (task.last_result == null) return undefined;
  if (task.last_result === 0) {
    return "The machine's Task Scheduler last result code: 0 (success).";
  }
  return `The machine's Task Scheduler last result code: ${task.last_result} (${taskResult(task.last_result)}).`;
}

/**
 * The repair task's value when the frame carries no fact for it, naming the build that sent it.
 *
 * This is a statement about the frame, not about the task: the build predates the fact or the
 * machine could not answer, and the console cannot tell which (so the hover says both). The version
 * comes from the fleet's running-version field and is drawn only when it exists — an online machine
 * that announced no build keeps the sentence and gains no invented parenthesis.
 */
function absentRepairTaskSentence(version: string | null | undefined): string {
  const build = typeof version === "string" ? version.trim() : "";
  return build === "" ? "not reported by this build" : `not reported by this build (v${build})`;
}

/** The day and the console's clock, the pair the update card uses to state a moment. */
function taskMoment(ms: number): string {
  return `${isoDate(ms)} ${clockTime(ms)}`;
}

/** A Task Scheduler result: `0` for success, otherwise the zero-padded hexadecimal code. */
function taskResult(result: number): string {
  if (result === 0) return "0";
  return `0x${(result >>> 0).toString(16).toUpperCase().padStart(8, "0")}`;
}

/**
 * The helper row's value, in the fleet's own fact.
 *
 * The fleet reports whether a per-user helper registered this boot, not whether its process is
 * running: a helper process can exist without ever registering a port, and an old device reports no
 * helper fact at all. So `registered: false` says `not registered this boot` — the Screen page's
 * "has not registered" — and never `not running`, a process state the console did not read. A
 * device too old to report the flag is `registration not reported`, and a device with no helper
 * fact is `not reported`. The registered row is unchanged (`session N`), and the pid/session are
 * appended only when the device actually reported them: the fleet's unregistered fact carries
 * neither, so no row prints `pid null` or invents a `?` for it.
 */
function helperSentence(helper: HostFacts["helper"]): string {
  if (!helper) return "not reported";
  if (helper.registered === true) return `session ${helper.session ?? "?"}`;
  const state =
    helper.registered === false ? "not registered this boot" : "registration not reported";
  const reported = [
    helper.pid != null ? `pid ${helper.pid}` : null,
    helper.session != null ? `session ${helper.session}` : null
  ].filter((part): part is string => part !== null);
  return reported.length > 0 ? `${state} · ${reported.join(" · ")}` : state;
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

export function gib(bytes?: number): string {
  if (!bytes) return "0 GiB";
  return `${(bytes / 1024 ** 3).toFixed(1)} GiB`;
}
