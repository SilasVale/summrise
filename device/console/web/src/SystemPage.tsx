import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import type { Device } from "./DeviceCard";
import { spanText } from "./age";
import { clockTime, grouped, isoDate, lastHeardMs, message, seenAgo } from "./format";
import { HostFactsBlock } from "./HostFacts";
import { Modal } from "./Modal";
import { useRequestGate } from "./requests";
import { callTool, ToolError, type Grant } from "./terminal";
import { RecoveryLine } from "./RecoveryLine";
import { StartedLine } from "./StartedLine";
import { updateRows } from "./updateCard";
import { useInstaller } from "./useInstaller";
import type { VitalsSample } from "./vitals";

/**
 * One running process, as `system.process_list` returns it.
 *
 * `memory_kb` is the resident set size the device could read: 0 is what the platform reports for a
 * process it will not open (and for a kernel thread), so the table shows the number rather than
 * pretending the process is weightless or omitted.
 */
type DeviceProcess = {
  pid: number;
  name: string;
  memory_kb: number;
};

/** A kill the platform refused, with the device's own reason. */
type KillFailure = { pid: number; reason: string };

/** The processes the device refuses to end; the row says so instead of offering a dead action. */
const PROTECTED_PROCESS_NAMES = new Set([
  "system",
  "system idle process",
  "registry",
  "memory compression",
  "secure system",
  "smss.exe",
  "csrss.exe",
  "wininit.exe",
  "services.exe",
  "lsass.exe",
  "winlogon.exe",
  "systemd",
  "init",
  "launchd",
  "kthreadd"
]);

function protectedProcess(process: DeviceProcess): boolean {
  return (
    process.pid === 0 ||
    process.pid === 1 ||
    process.pid === 4 ||
    PROTECTED_PROCESS_NAMES.has(process.name.toLowerCase())
  );
}

/** What `system.process_kill` did: the pids it signalled and the ones it could not. */
type KillReport = { killed: number[]; failed: KillFailure[] };

/**
 * What `system.net_test` established.
 *
 * `connected` is null when no address answered, and `error` then carries the device's reason. That
 * is a result, not a failed call: the name resolved, the port refused.
 */
type NetReport = {
  addresses: string[];
  connected: string | null;
  latency_ms: number | null;
  error: string | null;
};

/** A kill report kept with the row it was confirmed for, because the table may have moved on. */
type KillOutcome = { target: DeviceProcess; report: KillReport };

/** A test's report kept with the target it ran against, because the form may have moved on. */
type NetResult = { host: string; port: number } & NetReport;

/// `system.net_test`'s default and ceiling, mirrored from the device so the form states them.
const DEFAULT_NET_TIMEOUT_MS = 5_000;
const MAX_NET_TIMEOUT_MS = 60_000;

const READING_PROCESSES = "Reading the device's process list…";

/**
 * What a successful `host.restart` means, in the page's words.
 *
 * The tool answers before the app exits, so this is the fact the device sent, not a promise that it
 * is back: the reconnection is the launcher's, and the sentence says "restarting".
 */
const RESTARTED_SENTENCE =
  "The device is restarting through its launcher. It will reconnect in a few seconds; " +
  "terminals, browser tabs and runs on the device ended.";

/**
 * The device's own update story, as the device's panel draws it: a labelled grid, not a sentence.
 *
 * The Machine tab used to answer "did the update land" with one 12 px grey paragraph after the
 * facts — `running v0.3.290 · channel canary · last check 01:51 · last outcome applied to 0.3.290 on
 * 00:22` — so the applied answer was the paragraph's last clause, the running version was written
 * three times on the first screen, and the target version twice in one line. The device's own panel
 * answers the same question with labelled rows (`Last target`, `Last outcome`, `Last result`, `Last
 * check`); this card is that reading, in the console's `card` + `facts` vocabulary.
 *
 * `updateRows` decides the rows and their order: the applied version leads when it is the one
 * running, the running version and the target get a row each when they differ, and a record from
 * before this start says so. The card draws no fact the fleet did not send — a story the device
 * never sent keeps the one honest sentence instead of a grid of absences.
 */
function UpdateCard({ device }: { device?: Device | null }) {
  const installer = useInstaller();
  if (!device?.update) {
    return (
      <p className="hint system-update">
        The console has no update facts from this device: it has not sent its update story.
      </p>
    );
  }
  // `undefined` while the read is out, `null` when the console has no staged installer, and the
  // release's version when it does. The card states the comparison only when it is a fact.
  const release =
    installer === null ? undefined : installer.available ? installer.version : null;
  const rows = updateRows(device, Date.now(), release);
  return (
    <section className="card system-update" aria-labelledby="system-update-title">
      <header>
        <strong id="system-update-title">Updates</strong>
      </header>
      <dl className="facts">
        {rows.map((row) => (
          <div key={row.key}>
            <dt>{row.label}</dt>
            <dd className={row.absent ? "muted" : undefined}>
              {row.exact ? <time title={row.exact}>{row.text}</time> : row.text}
            </dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

/**
 * The console link's own story, under the update line.
 *
 * **THE DEVICE'S RETRY NOTE IF IT REACHED THIS CONSOLE, ELSE THE AUDIT'S TALLY.** `last_attempt` is
 * the device's own `console_attempt` — the count of failed attempts before the channel that carried
 * it, and the device's sentence about why. It lives in this console's memory, so a console restarted
 * since the device last connected has forgotten it; the audit file has not, and `channel_activity`
 * is the joins and leaves it wrote in the same newest window the absence pair is read from. The
 * fallback states the lines that window actually read and the span they cover, so the tally can be
 * read as a rate and cannot read as all time; a window whose ends carry no moment is stated without
 * a duration. It then explains what a join and a left are. Neither facts present says so; nothing
 * here invents a retry.
 */
function ConsoleLinkLine({ device }: { device?: Device | null }) {
  if (!device) return null;
  const attempt = device.last_attempt;
  const attempts = attempt?.attempts ?? 0;
  const reason = attempt?.reason?.trim() ?? "";
  if (attempt && (attempts > 0 || reason !== "")) {
    const failed = `${attempts} failed attempt${attempts === 1 ? "" : "s"}`;
    if (attempts > 0) {
      return (
        <p className="hint system-channel">
          {`The device's last console attempt: retried after ${failed}${
            reason ? ` — ${reason}` : ""
          }.`}
        </p>
      );
    }
    return (
      <p className="hint system-channel">{`The device's last console attempt: ${reason}.`}</p>
    );
  }
  const activity = device.channel_activity;
  if (activity) {
    const joins = `${activity.joined} join${activity.joined === 1 ? "" : "s"}`;
    const leaves = `${activity.left} leave${activity.left === 1 ? "" : "s"}`;
    const lines = `${grouped(activity.window)} line${activity.window === 1 ? "" : "s"}`;
    // **THE SPAN IS THE WINDOW'S OWN.** The first and last lines the read returned give the
    // duration; when either carries no moment there is no span to state, so the sentence keeps the
    // lines and invents nothing.
    const span = activity.span_ms != null ? `, spanning ${spanText(activity.span_ms)}` : "";
    return (
      <p className="hint system-channel">
        {`The console's audit recorded ${joins} and ${leaves} for this machine in its newest ` +
          `${lines}${span}. A join is the console accepting this machine's channel; a ` +
          "left is the channel closing, and a join after a left is a reconnect."}
      </p>
    );
  }
  return (
    <p className="hint system-channel">
      The console has no record of this machine's channel attempts: its newest audit window holds no
      join or leave, and this console holds no retry note from it.
    </p>
  );
}

/**
 * The local door to an update check, since the console has none.
 *
 * The device's own panel carries "Check for updates now" and the console's tool registry has no
 * update-check tool, so the page names the check it cannot run rather than offering a button that
 * would be a lie (or an address that resolves on the operator's machine, not the device's).
 */
function UpdateCheckLine() {
  return (
    <p className="hint system-check">
      The console has no update-check tool: a check runs on the machine's own panel ("Check for
      updates now").
    </p>
  );
}

/**
 * Why this machine's facts are absent, with each party's moment labelled.
 *
 * The facts live in THIS console process's memory, so an empty section is a fact about the console
 * as much as about the machine. For an offline machine the console's restart explains why the
 * section is empty and the machine's own last-seen is the date that answers `how long ago`; stating
 * only the console's start put two contradictory absence dates on one page (audit-44 F6). An online
 * machine has been heard from, so only the console boundary applies: its frames simply carry no
 * machine facts.
 */
function missingFactsSentence(
  device: Device | null | undefined,
  consoleStartedAt: number | null | undefined
): string {
  const started =
    consoleStartedAt != null
      ? ` at ${isoDate(consoleStartedAt)} ${clockTime(consoleStartedAt)}`
      : "";
  const heard = device ? lastHeardMs(device) : null;
  const lastSeen = heard != null ? `it was last seen ${seenAgo(heard)}` : null;
  let lead: string;
  if (device && !device.online) {
    lead =
      consoleStartedAt != null
        ? `The console restarted${started} and has not heard from this machine since`
        : "The console has no machine facts from this device";
    lead += lastSeen ? `; ${lastSeen}.` : ".";
  } else {
    lead = `The console has no machine facts from this device since this console started${started}.`;
  }
  return (
    `${lead} They travel with the fleet read, which the console repeats every few seconds; a ` +
    "device that does not report them leaves this section with none."
  );
}

/**
 * The device's processes and its reachability, as a page.
 *
 * Both are device state, so this reads and acts through the same grant as the terminal, the browser,
 * memory, monitors and runs: `system.process_list` for a snapshot, `system.process_kill` for one
 * deliberate end, `system.net_test` for one TCP connect. There is no console route — every call is
 * signed by the console, checked by the device, and audited like the other panes' calls.
 *
 * A kill is not one click away from a typo: the row names the process, and the confirm names the pid
 * and that name again. A net test that reaches nothing is a result (the addresses resolved and the
 * device says why the connection failed), not an error page: the device answered.
 */
export function SystemPage({
  grant,
  device,
  deviceOnline,
  consoleStartedAt,
  vitals = []
}: {
  grant: Grant;
  /** The selected machine, from the fleet read; null while it is starting or gone. */
  device?: Device | null;
  /** The fleet's reachability, as the other device pages take it; `false` is the only fact. */
  deviceOnline?: boolean;
  /**
   * When the console process began, from `/health`'s uptime. The machine facts live in this
   * process's memory, so an empty This-machine section is a fact about THIS process's memory, not
   * about the device: the sentence dates the boundary instead of saying the device never sent any.
   * Absent when `/health` could not state it; the sentence then names the boundary without a time.
   */
  consoleStartedAt?: number | null;
  vitals?: VitalsSample[];
}) {
  const offline = deviceOnline === false;
  const [processes, setProcesses] = useState<DeviceProcess[]>([]);
  const [note, setNote] = useState(READING_PROCESSES);
  const [listError, setListError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const [filter, setFilter] = useState("");
  // The filter the table was actually read with. The device filters, so what is typed is not in
  // effect until a read carries it; this is what the note and a kill's re-read are built from.
  const [applied, setApplied] = useState("");

  const [killTarget, setKillTarget] = useState<DeviceProcess | null>(null);
  const [killError, setKillError] = useState<string | null>(null);
  const [killing, setKilling] = useState(false);
  const [outcome, setOutcome] = useState<KillOutcome | null>(null);

  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [timeoutMs, setTimeoutMs] = useState(String(DEFAULT_NET_TIMEOUT_MS));
  const [testing, setTesting] = useState(false);
  const [testingFor, setTestingFor] = useState<string | null>(null);
  const [netResult, setNetResult] = useState<NetResult | null>(null);
  const [netError, setNetError] = useState<string | null>(null);

  const [restartOpen, setRestartOpen] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [restarted, setRestarted] = useState(false);
  // The request may have reached the device even though the answer did not: the app can be exiting
  // while the connection carrying its answer disappears. Keeping the button disabled here is what
  // stops a reflex second click from spending another of the launcher's three restarts.
  const [restartUnknown, setRestartUnknown] = useState(false);
  const [restartError, setRestartError] = useState<string | null>(null);

  // A failed first read has nothing to keep showing; a failed refresh must leave the previous rows
  // where they are, so the two are told apart by whether a list ever arrived.
  const listed = useRef(false);
  // The disabled buttons are the visible guard, but a second submit can arrive before the re-render
  // disables them; the handler's `killing`/`testing` is the value from the render that created it,
  // so the refs are the real gate. The kill is destructive: two calls would be two audit lines and,
  // after the first one ended the pid, a second refusal for a process that is already gone.
  const killingRef = useRef(false);
  const testingRef = useRef(false);
  // A restart is the one call whose success ends the connection it answered on. One click must be
  // one call: the ref is the gate before the disabled button re-renders, and the rest of the page
  // stays usable while it is on the wire.
  const restartingRef = useRef(false);
  // The list, the kill and the test paint different things, so each has its own ticket: a refresh
  // must not discard a kill that is still on the wire, and a kill's re-read must not paint over a
  // newer refresh. A new grant begins a new epoch for all three; the effect below does that.
  const listGate = useRequestGate();
  const killGate = useRequestGate();
  const netGate = useRequestGate();
  const restartGate = useRequestGate();

  const read = useCallback(
    async (needle: string) => {
      const ticket = listGate.begin();
      setLoading(true);
      setListError(null);
      try {
        // The device applies the filter, case-insensitively on the executable name. Sending nothing
        // for an empty box is the whole list; a client-side filter would hide processes the device
        // would still kill by name.
        const args: Record<string, unknown> = {};
        if (needle !== "") args.name = needle;
        const rows = JSON.parse(
          await callTool(grant, "system.process_list", args)
        ) as DeviceProcess[];
        if (listGate.stale(ticket)) return;
        listed.current = true;
        setProcesses(rows);
        setApplied(needle);
        setNote(listNote(rows, needle));
      } catch (cause) {
        if (listGate.stale(ticket)) return;
        // The rows stay as they were: a refresh that fails should not blank a list the operator can
        // still read, and the sentence says what the device refused.
        if (!listed.current) setNote("The device's process list could not be read.");
        setListError("Could not read the process list: " + message(cause));
      } finally {
        if (!listGate.stale(ticket)) setLoading(false);
      }
    },
    [grant, listGate]
  );

  // A different grant is a different device's processes and reachability, and a different draft. It
  // also invalidates a kill or a test still on the wire: its answer belongs to the device this page
  // has left.
  useEffect(() => {
    killGate.begin();
    netGate.begin();
    restartGate.begin();
    killingRef.current = false;
    testingRef.current = false;
    restartingRef.current = false;
    listed.current = false;
    setProcesses([]);
    setFilter("");
    setApplied("");
    setNote(READING_PROCESSES);
    setListError(null);
    setKillTarget(null);
    setKillError(null);
    setKilling(false);
    setOutcome(null);
    setHost("");
    setPort("");
    setTimeoutMs(String(DEFAULT_NET_TIMEOUT_MS));
    setTesting(false);
    setTestingFor(null);
    setNetResult(null);
    setNetError(null);
    setRestartOpen(false);
    setRestarting(false);
    setRestarted(false);
    setRestartError(null);
    // A MACHINE THE FLEET SAYS IS DOWN IS NOT ASKED for its processes; the page would sit on
    // "Reading the device's process list…" until the console's dial timed out.
    if (offline) {
      setNote("The machine is offline; its processes cannot be read until it reconnects.");
      return;
    }
    void read("");
  }, [read, killGate, netGate, restartGate, offline]);

  // The filter is sent, not applied here: pressing Refresh (or Enter) is the device call, and the
  // note says what the table is, not what the box says.
  const refresh = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    void read(filter.trim());
  };

  const confirmKill = async () => {
    if (!killTarget || killingRef.current) return;
    const target = killTarget;
    killingRef.current = true;
    const ticket = killGate.begin();
    setKilling(true);
    setKillError(null);
    setOutcome(null);
    try {
      // One target, one answer: a pid, never a pid and a name. The row's name is what the confirm
      // used to make the decision, but the request is the pid the device reported for it.
      const report = JSON.parse(
        await callTool(grant, "system.process_kill", { pid: target.pid })
      ) as KillReport;
      if (killGate.stale(ticket)) return;
      setKillTarget(null);
      setOutcome({ target, report });
    } catch (cause) {
      if (killGate.stale(ticket)) return;
      // The modal stays open: the device's sentence belongs next to the process it was about, and
      // the operator can cancel or try again after reading it.
      setKillError(
        "The device did not end PID " + target.pid + " (" + target.name + "): " + message(cause)
      );
    } finally {
      if (!killGate.stale(ticket)) {
        killingRef.current = false;
        setKilling(false);
      }
    }
    if (killGate.stale(ticket)) return;
    // The list re-reads after every attempt, report or refusal: a console timeout can lose the
    // answer to a kill that landed, and only the list says whether the process is still there.
    await read(applied);
  };

  const test = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (testingRef.current) return;
    testingRef.current = true;
    const ticket = netGate.begin();
    const wantedHost = host.trim();
    const wantedPort = Number(port);
    const args: Record<string, unknown> = { host: wantedHost, port: wantedPort };
    const wantedTimeout = Number(timeoutMs);
    if (timeoutMs.trim() !== "" && Number.isFinite(wantedTimeout)) {
      // An empty box means the device's default (5000); a value it refuses is still sent, so the
      // refusal is in the device's words rather than the browser's. The device clamps it.
      args.timeout_ms = wantedTimeout;
    }
    const target = where(wantedHost, wantedPort);

    setTesting(true);
    setTestingFor(target);
    setNetError(null);
    try {
      const report = JSON.parse(
        await callTool(grant, "system.net_test", args)
      ) as NetReport;
      if (netGate.stale(ticket)) return;
      setNetResult({ host: wantedHost, port: wantedPort, ...report });
    } catch (cause) {
      if (netGate.stale(ticket)) return;
      // A refusal here is the device declining the question (a name that does not resolve, a host
      // that is not a name); a connection that failed is a report and never reaches this branch.
      // The last result stays where it is, labelled as the previous one.
      setNetError("Could not test " + target + ": " + message(cause));
    } finally {
      if (!netGate.stale(ticket)) {
        testingRef.current = false;
        setTesting(false);
        setTestingFor(null);
      }
    }
  };

  /**
   * One deliberate restart, and then no second one from this page.
   *
   * The device builds `{"restarting": true}`, starts the app's exit, and answers; the connection that
   * carried the answer will go away with the app. The page therefore reports the fact it was given
   * and leaves the reconnection to the launcher. A success disables the button until a reload: the
   * launcher's budget is three consecutive restarts, and spending it is not a thing to do twice by
   * reflex. A refusal is the device's sentence, and the button stays available because nothing
   * ended.
   */
  const confirmRestart = async () => {
    if (restartingRef.current || restarted || restartUnknown) return;
    restartingRef.current = true;
    const ticket = restartGate.begin();
    // The confirmation has made its decision; the outcome is kept in the section, where it stays
    // legible after the dialog is gone.
    setRestartOpen(false);
    setRestarting(true);
    setRestartError(null);
    try {
      const answer = JSON.parse(await callTool(grant, "host.restart", {})) as {
        restarting?: unknown;
      };
      if (restartGate.stale(ticket)) return;
      if (answer.restarting !== true) {
        throw new Error("the device answered without confirming the restart");
      }
      setRestarted(true);
    } catch (cause) {
      if (restartGate.stale(ticket)) return;
      // 0 is "no answer at all"; 502/503/504 are the console's own transport answers. In all four the
      // request may already have restarted the app, so the page stops offering a second one.
      if (cause instanceof ToolError && [0, 502, 503, 504].includes(cause.status)) {
        setRestartUnknown(true);
        setRestartError(
          "The connection ended before the device confirmed the restart. It may be restarting; " +
            "reload the page once it reconnects instead of asking again."
        );
      } else {
        setRestartError("The device did not restart: " + message(cause));
      }
    } finally {
      if (!restartGate.stale(ticket)) {
        restartingRef.current = false;
        setRestarting(false);
      }
    }
  };

  const listedFilter = filter.trim();
  const filterPending = listedFilter !== applied;

  // **AN OFFLINE BLOCK IS DATED BY THE NEWEST MOMENT THE FLEET CAN VOUCH FOR.** A frame is what the
  // values came from and wins when the console has one; a console that restarted mid-outage has no
  // `last_frame_ms` (the measured 14R) and the registry's `last_seen_ms` is the only moment left.
  // Which fact dated it travels with the moment: "last frame" and "last seen" are different claims.
  const factsReadAt = offline ? (device?.last_frame_ms ?? device?.last_seen_ms ?? null) : null;
  const factsReadFrom = device?.last_frame_ms != null ? "frame" : "seen";

  return (
    <div className="system-page">
      <section className="system-section" aria-labelledby="system-this-machine">
        <h3 id="system-this-machine">This machine</h3>
        {device?.host ? (
          <HostFactsBlock
            host={device.host}
            vitals={vitals}
            collecting={!offline}
            readAtMs={factsReadAt}
            readFrom={factsReadFrom}
            online={device.online}
            runningVersion={device.version}
          />
        ) : (
          // **ONE MACHINE DATE, ONE CONSOLE DATE, EACH LABELLED (audit-44 F6).** The measured
          // offline page said the console had no facts "since this console started at 09:52" beside
          // the notice's "Last seen 2d 18h ago"; 09:52 was this process's start, not the machine's
          // last fact, and the two dates read as a contradiction. An offline machine now gets both
          // facts — the console restart and the machine's own last seen — and the last seen is the
          // date the card answers with.
          <p className="hint">{missingFactsSentence(device, consoleStartedAt)}</p>
        )}
        {device && (
          // **THE REACHABILITY WORD NOW HAS A SUBJECT.** The facts card ended on a bare `online`
          // (audit-40 F3): a 12 px orphan under the chart, reading as a label with no value or a
          // value with no label. It is the machine's status, so the line names it; the topbar still
          // carries the running chip, and the offline notice still explains the silence.
          <p className="hint system-status">
            <span className="muted">Status</span> ·{" "}
            <span className={device.online ? "ok" : undefined}>
              {device.online ? "online" : "offline"}
            </span>
          </p>
        )}
      </section>

      <section className="system-section" aria-labelledby="system-device-app">
        <h3 id="system-device-app">Device app</h3>
        {/* The machine's start line belongs to the section that answers what the app is doing
            (audit-44 F1). The update card below states the running version and the updater's last
            answer, so the line keeps only the moment the process started. The recovery line is its
            companion: the gap before this start, when a record proves one (audit-44 F12). */}
        {device && <StartedLine device={device} updateShown />}
        <RecoveryLine device={device} consoleStartedAt={consoleStartedAt} />
        <UpdateCard device={device} />
        {device && (
          <>
            <ConsoleLinkLine device={device} />
            <UpdateCheckLine />
          </>
        )}
        <p className="hint">
          The app the launcher supervises. Restarting it ends every open terminal, browser tab and
          run on the device, and takes the device offline briefly while the launcher starts it again.
        </p>
        <div className="system-toolbar">
          <button
            type="button"
            className="danger"
            disabled={restarting || restarted || restartUnknown || offline}
            title={offline ? "The machine is offline; it can restart when it reconnects" : undefined}
            onClick={() => {
              setRestartError(null);
              setRestartOpen(true);
            }}
          >
            Restart app
          </button>
        </div>
        {restarting && <p className="hint">Asking the device to restart…</p>}
        {restartError && <p className="error">{restartError}</p>}
        {restarted && (
          <>
            <p>{RESTARTED_SENTENCE}</p>
            <p className="hint">
              This page will not offer another restart: reload it once the device reconnects.
            </p>
          </>
        )}
      </section>

      <section className="system-section" aria-labelledby="system-processes">
        <h3 id="system-processes">Processes</h3>
        <form className="system-toolbar" onSubmit={refresh}>
          <div className="system-field filter">
            <label htmlFor="system-filter">Filter</label>
            <input
              id="system-filter"
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              placeholder="process name contains…"
              autoComplete="off"
              spellCheck={false}
            />
          </div>
          <button
            type="submit"
            disabled={loading || offline}
            title={offline ? "The machine is offline; the list refreshes when it reconnects" : undefined}
          >
            {loading ? "Reading…" : "Refresh"}
          </button>
        </form>
        <p className="hint">{note}</p>
        {filterPending && (
          <p className="hint">
            The device applies the filter, so the table still shows the previous read. Press Refresh
            (or Enter) to ask it for this one.
          </p>
        )}
        {listError && <p className="error">{listError}</p>}

        {outcome && (
          <section className="card system-kill">
            <header>
              <strong>Kill result</strong>
              <div className="actions">
                <button type="button" className="mini" onClick={() => setOutcome(null)}>
                  Dismiss
                </button>
              </div>
            </header>
            {/* Failures come first: a partly failed kill must open with what did not happen, and
                the success line is only for the pids the device says it ended. */}
            {outcome.report.failed.length > 0 && (
              <>
                <p className="error">
                  {outcome.report.failed.length === 1 &&
                  outcome.report.failed[0].pid === outcome.target.pid
                    ? `The device did not end PID ${outcome.target.pid} (${outcome.target.name}).`
                    : `The device did not end every process it was given.`}
                </p>
                <ul className="system-failures">
                  {outcome.report.failed.map((failure) => (
                    <li key={failure.pid}>
                      <span className="mono">PID {failure.pid}</span>: {failure.reason}
                    </li>
                  ))}
                </ul>
              </>
            )}
            {outcome.report.killed.length > 0 && (
              <p>{killedSentence(outcome.target, outcome.report.killed)}</p>
            )}
            {outcome.report.killed.length === 0 && outcome.report.failed.length === 0 && (
              <p className="hint">
                The device returned neither a pid it ended nor one it refused, which is not a
                result this page knows how to read.
              </p>
            )}
          </section>
        )}

        {processes.length > 0 && (
          <div className="scroll system-table">
            <table>
              <thead>
                <tr>
                  <th>PID</th>
                  <th>Process</th>
                  <th>Memory</th>
                  <th>Action</th>
                </tr>
              </thead>
              <tbody>
                {processes.map((process) => (
                  <tr key={process.pid}>
                    <td className="system-pid">{process.pid}</td>
                    <td className="system-name">{process.name}</td>
                    <td
                      className="system-memory"
                      title={`${process.memory_kb} KiB resident, as the device reported it`}
                    >
                      {megabytes(process.memory_kb)}
                    </td>
                    <td className="system-kill-cell">
                      {protectedProcess(process) ? (
                        <span className="hint" title="The device refuses to end an operating-system process">
                          Protected
                        </span>
                      ) : (
                        <button
                          type="button"
                          className="mini danger"
                          disabled={killing}
                          aria-label={`Kill PID ${process.pid} (${process.name})`}
                          onClick={() => {
                            setKillError(null);
                            setKillTarget(process);
                          }}
                        >
                          Kill
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section className="system-section" aria-labelledby="system-net">
        <h3 id="system-net">Network test</h3>
        <p className="hint">
          One TCP connect from the device to one port — the question its own clients ask, not an
          ICMP ping. A port that refuses the connection is a result: the addresses are what the
          device resolved, and the reason sits with them. A name that does not resolve is a refusal
          and is shown as one.
        </p>
        <form className="system-net" onSubmit={(event) => void test(event)}>
          <div className="system-field host">
            <label htmlFor="system-host">Host</label>
            <input
              id="system-host"
              value={host}
              onChange={(event) => setHost(event.target.value)}
              placeholder="192.168.1.1 or example.com"
              autoComplete="off"
              spellCheck={false}
              required
            />
          </div>
          <div className="system-field">
            <label htmlFor="system-port">Port</label>
            <input
              id="system-port"
              type="number"
              min={1}
              max={65535}
              value={port}
              onChange={(event) => setPort(event.target.value)}
              placeholder="443"
              required
            />
          </div>
          <div className="system-field">
            <label htmlFor="system-timeout">Timeout (ms)</label>
            <input
              id="system-timeout"
              type="number"
              min={1}
              max={MAX_NET_TIMEOUT_MS}
              value={timeoutMs}
              onChange={(event) => setTimeoutMs(event.target.value)}
            />
          </div>
          <button type="submit" className="primary" disabled={testing || offline}>
            {testing ? "Testing…" : "Test"}
          </button>
        </form>
        <p className="hint">
          The timeout is 1 to {MAX_NET_TIMEOUT_MS} ms and defaults to {DEFAULT_NET_TIMEOUT_MS} ms
          when the field is empty. It covers resolving and connecting together, and the device
          clamps it.
        </p>

        {testingFor && (
          <p className="hint">
            Testing {testingFor}…
            {netResult ? " The result below is from the previous test." : ""}
          </p>
        )}
        {netError && <p className="error">{netError}</p>}
        {netError && netResult && (
          <p className="hint">
            The result below is from the last test that finished; this attempt did not replace it.
          </p>
        )}

        {netResult && (
          <section className="card system-net-result">
            <header>
              <strong className="mono">{where(netResult.host, netResult.port)}</strong>
              <span className={"chip " + (netResult.connected ? "chip-ok" : "chip-warn")}>
                {netResult.connected ? "connected" : "not connected"}
              </span>
            </header>
            <dl className="facts">
              <div>
                <dt>Addresses</dt>
                <dd className="mono">{netResult.addresses.join(", ")}</dd>
              </div>
              <div>
                <dt>Connected</dt>
                <dd className="mono">{netResult.connected ?? "none"}</dd>
              </div>
              <div>
                <dt>Latency</dt>
                <dd>{netResult.latency_ms == null ? "none" : `${netResult.latency_ms} ms`}</dd>
              </div>
              {netResult.error && (
                <div>
                  <dt>Reason</dt>
                  <dd>{netResult.error}</dd>
                </div>
              )}
            </dl>
          </section>
        )}
      </section>

      {restartOpen && (
        <Modal
          title="Restart the device app?"
          onClose={() => {
            if (restartingRef.current) return;
            setRestartOpen(false);
          }}
        >
          <p>
            This asks the device to restart its app through the launcher. Every open terminal,
            browser tab and run on the device ends, and the device goes offline briefly while the
            launcher starts it again.
          </p>
          <p className="hint">
            The launcher allows three consecutive restarts: do not restart repeatedly, because three
            quick restarts can leave the device down. The device audits the call, and it refuses when
            it was not started by the launcher.
          </p>
          <div className="modal-actions">
            {/* Cancel is first so the dialog opens on the way out, not on the destructive call. */}
            <button type="button" onClick={() => setRestartOpen(false)}>
              Cancel
            </button>
            <button
              type="button"
              className="danger"
              disabled={restarting}
              onClick={() => void confirmRestart()}
            >
              Restart app
            </button>
          </div>
        </Modal>
      )}

      {killTarget && (
        <Modal
          title={`Kill PID ${killTarget.pid} (${killTarget.name})?`}
          onClose={() => {
            if (killingRef.current) return;
            setKillTarget(null);
            setKillError(null);
          }}
        >
          <p>
            This asks the device to end <span className="mono">PID {killTarget.pid}</span>,{" "}
            {killTarget.name}, now. It cannot be undone, and a pid on its own does not identify a
            process: a reused pid would be a different one, so confirm both.
          </p>
          <p className="hint">
            The device audits the call. A process the platform will not let the device signal is
            reported with its reason instead of ending.
          </p>
          {killError && <p className="error">{killError}</p>}
          <div className="modal-actions">
            <button
              type="button"
              disabled={killing}
              onClick={() => {
                setKillTarget(null);
                setKillError(null);
              }}
            >
              Cancel
            </button>
            <button
              type="button"
              className="danger"
              disabled={killing}
              onClick={() => void confirmKill()}
            >
              {killing ? "Killing…" : `Kill PID ${killTarget.pid}`}
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}

/// The sentence for a kill's `killed` list, naming the process only when the pid is the one the
/// confirm named. A kill by name can answer several pids; this page always sends one pid.
function killedSentence(target: DeviceProcess, killed: number[]): string {
  if (killed.length === 1) {
    const pid = killed[0];
    return pid === target.pid
      ? `The device ended PID ${pid} (${target.name}).`
      : `The device ended PID ${pid}.`;
  }
  return `The device ended ${killed.map((pid) => "PID " + pid).join(", ")}.`;
}

/// "82.3 MB" — resident memory as the table reads it. The device reports KiB, so the conversion is
/// the page's, and the title keeps the number the device actually sent.
function megabytes(kb: number): string {
  return (kb / 1024).toFixed(1) + " MB";
}

/// What the table is, in the device's terms: the filter was applied there, so the note says how
/// many processes answered it rather than what the box contains.
function listNote(rows: DeviceProcess[], filter: string): string {
  if (rows.length === 0) {
    return filter === ""
      ? "The device reports no running processes."
      : `No process on the device has a name containing ${JSON.stringify(filter)}, ` +
          "case-insensitively.";
  }
  const noun = rows.length === 1 ? "process" : "processes";
  if (filter === "") return `${rows.length} ${noun} on the device, by pid.`;
  return (
    `${rows.length} ${noun} whose name contains ${JSON.stringify(filter)}, ` +
    "case-insensitively, by pid."
  );
}

/// `host:port`, bracketing a bare IPv6 host so the port is legible; a host that already carries its
/// brackets is not bracketed twice.
function where(host: string, port: number): string {
  const bare = host.replace(/^\[(.*)\]$/, "$1");
  return (bare.includes(":") ? `[${bare}]` : bare) + ":" + port;
}
