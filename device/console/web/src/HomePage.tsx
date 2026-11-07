import { useEffect, useState } from "react";
import { api, type Device, type Installer } from "./api";
import { DeviceCard } from "./DeviceCard";
import { copyText } from "./copy";
import { lastHeardMs, seenAgo, shortId } from "./format";
import { bytes } from "./runFormat";
import { useLiveTools } from "./useLiveTools";
import { versionBelow } from "./versions";

/** A one-time enrollment code and the command that redeems it. */
export type Enrollment = { code: string; expires_at_ms: number } | null;

/**
 * What the console knows about the machine list, which is not the same as the list it holds.
 *
 * `reading` is before the first answer, `failed` is an answer that did not arrive — with the reason
 * the console gave — and `ready` is a successful read. Only `ready` can make an empty list mean
 * "no machines yet": the other two are why the page must not say it.
 */
export type FleetRead =
  | { state: "reading" }
  | { state: "failed"; reason: string }
  | {
      state: "ready";
      /**
       * When the answer landed. Home states the fleet read's age beside "Nothing needs
       * attention."; absent is a page that was given the list without a clock (tests, older
       * callers), and then it says "just now" rather than inventing a moment.
       */
      at?: number;
    };

type FleetProps = {
  devices: Device[];
  /**
   * What the latest `/fleet` read answered. Absent is a confirmed answer for the machines in hand,
   * which is what a page that was not given a read state has.
   */
  fleetRead?: FleetRead;
  /** Read the machine list again after a failed read: the page's Retry, not the shell's poll. */
  onRetryFleet?: () => void;
  selectedId: string | null;
  code: Enrollment;
  enrollCommand: string;
  busy: boolean;
  onOpen: (id: string) => void;
  onRename: (device: Device) => void;
  onRemove: (device: Device) => void;
  onDiagnostics: (device: Device) => void;
  onAddMachine: () => void;
  /**
   * The fleet's credential door: open the existing Connect sheet on its Tokens section. The header
   * and the Add-a-machine panel both use it, so a person adding a machine meets the token there.
   */
  onTokens: () => void;
  onDismissCode: () => void;
  onNotice: (sentence: string) => void;
};

/**
 * The installer the console was given, read once when the fleet page is drawn.
 *
 * `null` is "nothing to show": the read has not landed, or it failed. That read is the installer's,
 * not `/fleet`'s: a slow or failed machine list must not take away the file that installs a machine.
 * A failed installer read hides the block rather than drawing a door that cannot open, and the page
 * does not poll it — the next visit to the fleet is the next read.
 */
function useInstaller(): Installer | null {
  const [installer, setInstaller] = useState<Installer | null>(null);
  useEffect(() => {
    let live = true;
    api
      .installer()
      .then((answer) => {
        if (live) setInstaller(answer);
      })
      .catch(() => {
        if (live) setInstaller(null);
      });
    return () => {
      live = false;
    };
  }, []);
  return installer;
}

/**
 * Where to get the device runtime: a compact strip under the fleet, not the page's first card.
 *
 * Every value is the `/installer` answer: the filename and version are the console's staged file,
 * the size and short hash are over the bytes the link serves. An unavailable installer is one
 * sentence and no link — the page offers no download it cannot honour. The block sits after the
 * fleet because the fleet is the page's subject; the installer is the next step once a machine is
 * missing, and it keeps its sha256 line so the file can be checked after it lands.
 */
function InstallerBlock(props: { installer: Installer; onNotice: (sentence: string) => void }) {
  if (!props.installer.available) {
    return (
      <p className="hint installer-missing">
        No installer is published for this console yet. Install it from the release, then add the
        machine for its enrollment command.
      </p>
    );
  }
  const installer = props.installer;
  return (
    <section
      className="installer installer-strip"
      aria-label="Install the device runtime"
    >
      <h2>Install the device runtime</h2>
      <a className="btn primary" href="/installer/download" download>
        Download Windows installer
      </a>
      <p className="hint">
        {installer.filename} · {bytes(installer.size_bytes)} ·{" "}
        {/* **THE DISPLAYED HASH IS A PREFIX; THE CHECK IS THE FULL DIGEST.** The strip showed
            `sha256 0adcbd7c…` with no title and no copy, so the one executable the console hands
            out could not be verified from the page. The full 64 hex is the title and one press
            away; `/installer` is no longer knowledge the operator has to have. */}
        <span className="mono" title={installer.sha256}>
          sha256 {installer.sha256.slice(0, 8)}…
        </span>
      </p>
      <button
        type="button"
        className="mini installer-digest"
        onClick={() =>
          void copyText(installer.sha256).then((ok) =>
            props.onNotice(ok ? "Digest copied." : "Could not copy the digest.")
          )
        }
      >
        Copy digest
      </button>
    </section>
  );
}

/** One thing on the fleet that wants a person: a machine that is away, or one behind the file. */
type AttentionProblem = {
  /** Stable key: one machine can have two problems, and both are listed under its one row. */
  key: string;
  device: Device;
  tone: "offline" | "behind";
  /**
   * The problem without its machine's name: the row's heading is the name, so repeating it in every
   * line would make one machine's two problems read as two machines (audit-40 F6).
   */
  clause: string;
};

/** One machine's problems, as the block draws them: a heading, its lines, one door. */
type AttentionMachine = {
  device: Device;
  /** The row's dot: away outranks behind when a machine has both. */
  tone: "offline" | "behind";
  problems: AttentionProblem[];
};

/**
 * What needs attention, from the fleet read and the staged installer only.
 *
 * The clauses are the brief's own: a machine that is away is dated by the newest evidence the
 * console has (the row's rule), and a machine behind the staged installer names both versions. A
 * machine can produce two problems — away and behind at once — and both are listed under that
 * machine's one row. Nothing here is invented: no evidence means the clause stops before "last
 * seen", and no staged installer means no comparison at all.
 */
function attentionProblems(
  devices: readonly Device[],
  installer: Installer | null
): AttentionProblem[] {
  const problems: AttentionProblem[] = [];
  for (const device of devices) {
    if (!device.online) {
      const heard = lastHeardMs(device);
      problems.push({
        key: `${device.id}:offline`,
        device,
        tone: "offline",
        clause: heard === null ? "Offline." : `Offline — last seen ${seenAgo(heard)}.`
      });
    }
    if (installer?.available && versionBelow(device.version, installer.version)) {
      problems.push({
        key: `${device.id}:behind`,
        device,
        tone: "behind",
        // THE INSTALLER'S VERSION IS THE RELEASE, NOT THE CONSOLE'S BUILD. The fleet's chip and the
        // Account menu name the console (`0.3.273-ux99`); this number is the staged
        // `SummriseSetup-0.3.290.exe`. Calling it "the console has 0.3.290" made one word mean two
        // versions on one screen (audit-40 F10).
        clause: `Behind: v${device.version}, the release is ${installer.version}.`
      });
    }
  }
  return problems;
}

/** The problems folded under the machine they belong to, in the fleet's own order. */
function attentionMachines(problems: readonly AttentionProblem[]): AttentionMachine[] {
  const groups = new Map<string, AttentionMachine>();
  for (const problem of problems) {
    let group = groups.get(problem.device.id);
    if (!group) {
      group = { device: problem.device, tone: problem.tone, problems: [] };
      groups.set(problem.device.id, group);
    }
    // A machine that is away and behind gets the away dot: the silence is the stronger fact.
    if (problem.tone === "offline") group.tone = "offline";
    group.problems.push(problem);
  }
  return [...groups.values()];
}

/**
 * The page's one sentence about the fleet: online count and how many machines need a person.
 *
 * "Needs attention" counts machines, not rows: an offline machine that is also behind is one
 * machine to fix. A machine behind the staged installer counts even while it is online — it is the
 * one fact on this page that means an action without a failed connection. When nothing is wrong
 * the sentence is the operator's calm one: "nothing to do".
 */
function verdictSentence(
  devices: readonly Device[],
  problems: readonly AttentionProblem[]
): string {
  const online = devices.filter((device) => device.online).length;
  const needing = new Set(problems.map((problem) => problem.device.id)).size;
  const attention =
    needing === 0
      ? "nothing to do"
      : `${needing} ${needing === 1 ? "needs" : "need"} attention`;
  return `${online} of ${devices.length} online · ${attention}`;
}

/** The fleet read's age, as the calm attention block states it. */
function fleetReadAge(fleet: FleetRead): string {
  return fleet.state === "ready" && fleet.at !== undefined
    ? `Fleet read ${seenAgo(fleet.at)}.`
    : "Fleet read just now.";
}

/**
 * Home's first answer: does anything need me?
 *
 * Non-empty, the block is one row per MACHINE: its name, every problem it has as a line under it,
 * and one door to open it. The verdict counts machines, so the rows must too — four rows and four
 * identical buttons over two machines made the count and the block disagree (audit-40 F6). Empty,
 * it says so once and dates the read that proved it, so "nothing to do" is evidence rather than a
 * blank. It is a named region either way, so a reader who navigates by landmark meets the page's
 * subject before the table.
 */
function AttentionBlock(props: {
  problems: readonly AttentionProblem[];
  fleet: FleetRead;
  onOpen: (id: string) => void;
}) {
  if (props.problems.length === 0) {
    return (
      <section className="attention attention-clear" aria-label="Needs attention">
        <p className="attention-clear-line">
          <span className="dot dot-ok" aria-hidden="true" />
          Nothing needs attention.
          <span className="hint"> {fleetReadAge(props.fleet)}</span>
        </p>
      </section>
    );
  }
  return (
    <section className="attention" aria-label="Needs attention">
      <h2>Needs attention</h2>
      <ul className="attention-list">
        {attentionMachines(props.problems).map((machine) => {
          const label = machine.device.name ?? shortId(machine.device.id);
          return (
            <li className="attention-row" key={machine.device.id}>
              <span
                className={"dot " + (machine.tone === "offline" ? "dot-off" : "dot-warn")}
                aria-hidden="true"
              />
              <div className="attention-body">
                <h3 className="attention-machine">{label}</h3>
                <ul className="attention-problems">
                  {machine.problems.map((problem) => (
                    <li key={problem.key}>{problem.clause}</li>
                  ))}
                </ul>
              </div>
              <button
                type="button"
                className="attention-open"
                aria-label={`Open machine for ${label}`}
                onClick={() => props.onOpen(machine.device.id)}
              >
                Open machine
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/**
 * Home's one section about the machine in context.
 *
 * **THE SELECTION IS A SECTION, NOT THE PAGE.** Choosing a machine used to swap the whole
 * destination for that machine's live page, which dropped the verdict and the attention rows — the
 * answer to Home's one question — and drew a second Activity under the rail's Home. The fleet stays
 * Home; this section says who the selected machine is, what it can do, and hands off to the
 * machine's own page, Device. It carries no day timeline: the record is Activity's.
 *
 * The facts are the fleet's own: the row's name, reachability, version and live catalogue counts.
 * It does not repeat the start line, which the table's row already states, and it never invents a
 * count while the catalogue is unread or the machine is away.
 */
function SelectedMachine(props: {
  device: Device;
  onTools: () => void;
  onOpenDevice: () => void;
  onConnect: () => void;
}) {
  const device = props.device;
  const name = device.name ?? shortId(device.id);
  // The live catalogue is a machine fact, not activity: it answers what this machine can do. The
  // hook is shared with the Overview tab, so the read is cached for both.
  const live = useLiveTools(device.id);
  const offline = !device.online || live.offline;
  const heard = lastHeardMs(device);
  const state =
    offline
      ? heard === null
        ? "Offline"
        : `Offline · last seen ${seenAgo(heard)}`
      : "Running";
  const capabilities = new Set(live.tools.map((tool) => tool.capability)).size;
  const countsKnown = !offline && !(live.error && live.tools.length === 0);
  return (
    <section className="machine-summary" aria-label={`Selected machine ${name}`}>
      <div className="machine-summary-head">
        <h2>{name}</h2>
        <span className="machine-summary-state">
          <span
            className={"dot " + (offline ? "dot-off" : "dot-ok")}
            aria-hidden="true"
          />
          {state}
        </span>
      </div>
      {offline ? (
        <p className="hint machine-summary-counts">
          Tool counts are unavailable while the machine is offline.
        </p>
      ) : !countsKnown ? (
        <p className="hint machine-summary-counts">
          Tool counts are unavailable until the console can read the machine's list.
        </p>
      ) : live.loading && live.tools.length === 0 ? (
        <p className="hint machine-summary-counts">Reading the machine's tool list…</p>
      ) : (
        <button
          type="button"
          className="counts-line machine-summary-counts"
          onClick={props.onTools}
        >
          {plural(live.tools.length, "tool")} ·{" "}
          {plural(capabilities, "capability", "capabilities")}
        </button>
      )}
      <div className="machine-summary-actions">
        <button type="button" className="primary" onClick={props.onOpenDevice}>
          Open Device
        </button>
        <button type="button" onClick={props.onConnect}>
          Connect a client
        </button>
      </div>
    </section>
  );
}

/** `1 tool` / `6 tools`, in the one place Home states a count. */
function plural(count: number, noun: string, many = `${noun}s`): string {
  return `${count} ${count === 1 ? noun : many}`;
}

/**
 * Home, with or without a machine selected: the verdict, what needs attention, the selected
 * machine's summary when there is one, the fleet table, and the installer. The page answers "what
 * needs me?" before it lists anything; the fleet is a compact table of four facts, and the per-row
 * client-call counters are gone — that is Activity's noun.
 */
function FleetHome(props: HomePageProps) {
  const installer = useInstaller();
  const fleet = props.fleetRead ?? { state: "ready" as const };
  // **A ZERO NEEDS A READ BEHIND IT.** The empty fleet is a fact only when `/fleet` answered with
  // no machines; a page that is still reading, or whose read failed, holds an empty list too, and
  // telling that reader "No machines yet" invites enrollment for a fleet the console cannot see.
  const empty = fleet.state === "ready" && props.devices.length === 0;
  // The verdict and the attention rows are claims about the fleet now, so they wait for a
  // successful read. A failed read keeps the last list on screen and states the failure instead.
  const ready = fleet.state === "ready";
  const problems = ready ? attentionProblems(props.devices, installer) : [];
  // **THE BANNER NAMES THE FAILURE; THIS LINE ONLY SAYS WHAT IS MISSING.** When the read failed
  // there is no verdict to give, so the header draws none and the notice below states what the page
  // can show. A second failure sentence here made the page say one failure three times (audit-40
  // F4); the banner's title still carries the browser's cause.
  const verdict =
    fleet.state === "reading"
      ? "Reading the machine list…"
      : fleet.state === "failed"
        ? null
        : empty
          ? "No machines yet"
          : verdictSentence(props.devices, problems);
  return (
    <div className="page home-page">
      <header className="page-head">
        <div>
          <h1>Machines</h1>
          {verdict !== null && (
            <p className={"home-verdict" + (problems.length > 0 ? " has-attention" : "")}>
              {verdict}
            </p>
          )}
        </div>
        <div className="page-actions">
          <button type="button" onClick={props.onTokens}>
            Tokens
          </button>
          {/* **A READ IN FLIGHT IS NOT A REASON TO TAKE THE DOOR AWAY.** The fleet's read and the
              enrollment are different subjects: adding a machine mints a code with a POST and never
              waits for `/fleet`. A slow or failing list is exactly when a person may be adding one,
              so the door is drawn in every read state; only the page's claim about the fleet waits. */}
          <button
            type="button"
            className="primary"
            disabled={props.busy}
            onClick={props.onAddMachine}
          >
            Add a machine
          </button>
        </div>
      </header>

      {fleet.state === "reading" && props.devices.length === 0 && (
        // The subtitle states it in words; the placeholder is the same fact for a reader who
        // navigates by region, in the shape the tool and plugin lists already use.
        <div className="skeleton-list" aria-label="Reading the machine list" role="status">
          {[0, 1].map((row) => (
            <div className="skeleton-row" key={row}>
              <span className="skeleton skeleton-name" />
              <span className="skeleton skeleton-line" />
            </div>
          ))}
        </div>
      )}

      {fleet.state === "failed" && (
        // ONE FAILURE, ONE SENTENCE, ONE RETRY. The shell's banner names the read and keeps the
        // browser's cause in its title; this notice says only what the page can and cannot show and
        // is the one control that reads it again (audit-40 F4). The cause stays a title here too,
        // so the evidence survives the banner's Dismiss.
        <div className="notice" role="status">
          <span title={fleet.reason}>
            {props.devices.length > 0
              ? "The machines below are the last list it read."
              : "The machine list is not shown."}
          </span>
          <button type="button" onClick={() => props.onRetryFleet?.()}>
            Retry
          </button>
        </div>
      )}

      {/* **WHAT NEEDS ME COMES BEFORE THE LIST.** The block only claims a problem on a successful
          read: a stale list below a failed read is not proof that a machine is away now. It is
          drawn with no machines neither — "Nothing needs attention" over an empty fleet would be
          true but useless, and the empty state below already says what to do. */}
      {ready && props.devices.length > 0 && (
        <AttentionBlock problems={problems} fleet={fleet} onOpen={props.onOpen} />
      )}

      {/* **THE MACHINE IN CONTEXT SITS BESIDE THE FLEET, NOT INSTEAD OF IT.** The verdict and the
          attention rows above are still the page's first answer; this section adds the selected
          machine's own facts and its door to Device. It is drawn whichever machine the fleet
          selected, including an offline one. */}
      {props.device !== null && (
        <SelectedMachine
          device={props.device}
          onTools={props.onTools}
          onOpenDevice={props.onDevice}
          onConnect={props.onConnect}
        />
      )}

      {props.devices.length > 0 ? (
        // A real table: the four fleet facts line up column-wise, and at 390 the stylesheet turns
        // each row into a card. The row's own link is its name; the overflow is the management.
        <div className="fleet-table-wrap">
          <table className="fleet-table" aria-label="The fleet">
            <thead>
              <tr>
                <th scope="col">Machine</th>
                <th scope="col">Status</th>
                <th scope="col">Version</th>
                <th scope="col">Started</th>
                <th scope="col">
                  <span className="sr-only">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {props.devices.map((device) => (
                <DeviceCard
                  key={device.id}
                  device={device}
                  selected={props.selectedId === device.id}
                  onSelect={() => props.onOpen(device.id)}
                  onRename={() => props.onRename(device)}
                  onRemove={() => props.onRemove(device)}
                  onDiagnostics={() => props.onDiagnostics(device)}
                />
              ))}
            </tbody>
          </table>
        </div>
      ) : empty ? (
        // **THE ZERO STATE IS ONE SENTENCE, AND THE DOORS ARE THE NEXT STEP.** `No machines yet`
        // already says the size, so `0 machines · 0 online` was the same fact twice. The old
        // paragraph sent the reader to "run the command" before the installer had been offered, and
        // the command's panel had no visible name; this block names the order (install, then add)
        // and both doors carry their own headings.
        <section className="empty-state first-run" aria-labelledby="first-run-title">
          <h2 id="first-run-title">Add your first machine</h2>
          <p className="hint">
            Start with the installer below, then press Add a machine for the enrollment command.
          </p>
        </section>
      ) : null}

      {/* The installer is the account's, not the read's: `/installer` is a separate answer, and it
          is the next step whether the machine list arrived, is still coming, or failed. It is a
          compact strip after the fleet, not the page's first card. */}
      {installer !== null && (
        <InstallerBlock installer={installer} onNotice={props.onNotice} />
      )}

      {props.code && (
        <section className="enroll" aria-labelledby="enroll-title">
          <h2 id="enroll-title">Enrollment command</h2>
          <p className="hint">
            This code works once, for 15 minutes. On the Windows machine, open an elevated Command
            Prompt in the folder you chose on the installer's directory page (the default is{" "}
            <code>C:\Program Files\Summrise</code>), run this command, then restart the{" "}
            <code>SummriseLaunch</code> service.
          </p>
          <pre className="code">{props.enrollCommand}</pre>
          <div className="actions">
            <button
              type="button"
              onClick={() =>
                void copyText(props.enrollCommand).then((ok) =>
                  props.onNotice(ok ? "Enrollment command copied." : "Could not copy the command.")
                )
              }
            >
              Copy command
            </button>
            <button type="button" onClick={props.onTokens}>
              Tokens
            </button>
            <button type="button" onClick={props.onDismissCode}>
              Dismiss
            </button>
          </div>
          <p className="hint">Expires at {new Date(props.code.expires_at_ms).toLocaleTimeString()}.</p>
        </section>
      )}
    </div>
  );
}

export type HomePageProps = FleetProps & {
  /// The machine in context. Null draws the fleet alone; a chosen machine adds its summary.
  device: Device | null;
  /** Open the selected machine's own page: Device → Overview. */
  onDevice: () => void;
  onConnect: () => void;
  onTools: () => void;
};

export function HomePage(props: HomePageProps) {
  // **ONE DESTINATION, ONE IDENTITY.** Every state is the fleet's page; a selected machine is a
  // section of it. The machine's own live page is Device → Overview, so Home never becomes it.
  return <FleetHome {...props} />;
}
