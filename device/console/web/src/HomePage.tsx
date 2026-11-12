import { readMonitors, type MonitorTarget } from "./monitorApi";
import { useEffect, useRef, useState } from "react";
import { type Device, type Installer } from "./api";
import { DeviceCard } from "./DeviceCard";
import { copyText } from "./copy";
import { presenceText, seenAgo, shortId } from "./format";
import { PAGE_LABELS } from "./Rail";
import { bytes } from "./runFormat";
import { useInstaller } from "./useInstaller";
import { useLiveTools } from "./useLiveTools";
import { fleetNewestVersion, releaseTarget, versionBelow, type Release } from "./versions";

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
  /** The fleet row's visible door: open that machine's own page, Device → Overview. */
  onOpen: (id: string) => void;
  /**
   * The attention row's visible door: open that machine where its last facts are, Device → Machine.
   * It is a separate verb from the fleet row's because the two blocks ask different questions —
   * "what is wrong with this machine?" and "which machine do I want?" — and the audit's measured
   * `Open machine` promised a destination it did not open (audit-43 F1).
   */
  onOpenMachine: (id: string) => void;
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
 * Where to get the device runtime: a compact strip under the fleet, not the page's first card.
 *
 * Every value is the `/installer` answer: the filename and version are the console's staged file,
 * the size and short hash are over the bytes the link serves. An unavailable installer is one
 * sentence and no link — the page offers no download it cannot honour. The block sits after the
 * fleet because the fleet is the page's subject; the installer is the next step once a machine is
 * missing, and it keeps its sha256 line so the file can be checked after it lands.
 */
function InstallerBlock(props: {
  installer: Installer;
  /** The fleet the strip is drawn beside: its newest build can already be past this file. */
  devices: readonly Device[];
  onNotice: (sentence: string) => void;
  /**
   * **BARE, AND WHY IT EXISTS (the fortieth list).** On a fleet of zero this block is the page's
   * NEXT STEP and not a footnote, so the first-run card hosts it. Hosting it framed would be a card
   * inside a card — the exact shape the thirty-eighth list removed from Settings — so the same
   * component draws its contents without its own box, and there is still only ONE place that knows
   * what the installer's three states look like.
   */
  bare?: boolean;
}) {
  if (!props.installer.available) {
    return (
      <p className="hint installer-missing">
        No installer is published for this console yet. Install it from the release, then add the
        machine for its enrollment command.
      </p>
    );
  }
  const installer = props.installer;
  // **AN INVITATION THE PAGE'S OWN NEXT LINE MAKES POINTLESS IS NOT AN INVITATION (the thirty-sixth
  // list).** Measured at 1440 on `0.3.273-ux137`: an accent-filled `Download Windows installer` sat
  // directly above `The fleet is already past this file: the newest build any machine reports is
  // 0.3.292.` — the page's loudest control inviting a press whose answer it had already given, and
  // the file could fix nothing on that fleet. The strip has TWO states now, and it says which one it
  // is in: a file the fleet is behind is an invitation (install it and the fleet catches up); a file
  // the fleet has outrun is a SPARE — what a machine that leaves and comes back, or a new one,
  // installs — and its download is a quiet secondary control rather than the page's call to action.
  // **A FLEET OF ZERO IS NEVER "STALE" (the fortieth list).** Measured on an empty fleet: the strip
  // said `Installer for a new or returning machine` and its note described a fleet already past the
  // file — about a fleet that does not exist. There is nothing to be behind.
  const stale = fleetBehindInstaller(props.devices, installer.version);
  const newest = fleetNewestVersion(props.devices);
  if (props.bare) {
    // The contents only: the first-run card owns the box, the heading and the sentence that names
    // the order. This is the same THREE-STATE body the strip draws everywhere else.
    return (
      <div className="first-run-door">
        <a className="btn primary" href="/installer/download" download>
          Download Windows installer
        </a>
        <p className="hint">
          {installer.filename} · {bytes(installer.size_bytes)} ·{" "}
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
      </div>
    );
  }
  return (
    <section
      className="card installer installer-strip"
      aria-label="Install the device runtime"
    >
      <h2>{stale ? "Installer for a new or returning machine" : "Install the device runtime"}</h2>
      <a
        className={stale ? "btn installer-spare" : "btn primary"}
        href="/installer/download"
        download
      >
        Download Windows installer
      </a>
      {/* **A STALE DOWNLOAD IS NOT THE TARGET (the thirty-third list).** The fleet ran 0.3.292
          while this staged installer was 0.3.291, and the attention block used to call that number
          "the release" — so the page's one downloadable file passed as the build everything should
          be measured against. When the machines are already past this file, its own line says so,
          AND what the file is still good for. */}
      {stale && (
        <p className="hint installer-stale">
          The fleet is already past this file: the newest build any machine reports is {newest}. It
          installs a machine that is not in the fleet yet, or one that comes back after being
          removed; a machine already running {newest} needs nothing from it.
        </p>
      )}
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
 * console has (the row's rule), and a machine behind the staged installer says so without naming
 * a version — the block states the release once, and the table's Version column holds the build.
 * A machine can produce two problems — away and behind at once — and both are listed under that
 * machine's one row. Nothing here is invented: no evidence means the clause stops before "last
 * seen", and no staged installer means no comparison at all.
 */
function attentionProblems(
  devices: readonly Device[],
  /** The build the behind rows are measured against, and where it came from. */
  release: Release | null
): AttentionProblem[] {
  const target = release?.version ?? null;
  const problems: AttentionProblem[] = [];
  for (const device of devices) {
    if (!device.online) {
      problems.push({
        key: `${device.id}:offline`,
        device,
        tone: "offline",
        // THE FLEET ROW'S OWN PHRASE, NOT A SECOND SPELLING. `offline · last seen 2d 16h ago` is
        // `presenceText`'s one casing; the old `Offline — last seen … .` made the same silence read
        // as a different fact in the block above the row that states it (audit-43 F2).
        clause: presenceText(device)
      });
    }
    if (target !== null && versionBelow(device.version, target)) {
      problems.push({
        key: `${device.id}:behind`,
        device,
        tone: "behind",
        // **THE ROW SAYS HOW FAR BEHIND, ONCE.** The block names the target release; this row names
        // the machine's own build, so a reader can see the gap without hunting the table 445 px
        // below (the twenty-eighth list, item 4). The number is the device's, not invented: it is
        // the `version` the fleet read returned, and it is absent (a plain `Behind the release.`)
        // when the machine did not report one.
        clause:
          device.version && target
            ? `On v${device.version} · behind ${target}.`
            : "Behind the release."
      });
    }
  }
  return problems;
}

/** Whether any machine reports a build newer than the installer on this page. */
function fleetBehindInstaller(devices: readonly Device[], installerVersion: string): boolean {
  return devices.some((device) => versionBelow(installerVersion, device.version));
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
  /** The build the behind rows are measured against, and where that number came from. */
  release: Release | null;
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
  // **ONE RELEASE SENTENCE, ONE VERSION POSITION.** The release is the installer's version, and
  // every behind row is measured against the same number; stating it in each row made one number
  // read as several facts. The rows name the machine and the problem; the block names the target.
  //
  // **BUT THE ROW MUST CARRY THE DISTANCE (the twenty-eighth list, item 4).** Measured on the live
  // page: the block read `The release is 0.3.291.` / `DESKTOP-14RJCR8` / `offline · last seen 4d 2h
  // ago` / `Behind the release.` / `Open`, and the machine's own version — `v0.3.275` — appeared
  // NOWHERE in the block: it was 445 px lower, in the table's Version column. A reader was told a
  // machine is behind and not how far, on the page whose whole job is the verdict and the next
  // action. The clause now names the two numbers that are already on the page, so the sentence is
  // the fact rather than a pointer to it.
  const behind = props.release !== null && props.problems.some((problem) => problem.tone === "behind");
  return (
    <section className="card attention" aria-label="Needs attention">
      {/* **AN EYEBROW IS NOT AN `h2` (the hundred-and-fifth list).** Measured 2026-11-11: this card drew an `h2` at
          **10 / 600, uppercase** — exactly the `section.eyebrow` atom's numbers — while the atom says `h2` is
          **17 / 600**. **The tag and the size disagreed, and the tag is what a screen reader hears**: the document
          outline announced a section heading, the page drew a small label. The card keeps its label and its
          `aria-label` (which is the accessible name the section already had); the outline stops inventing a level
          that is not drawn. */}
      <div className="attention-label">Needs attention</div>
      {behind && props.release && (
        // **THE SENTENCE SAYS WHICH NUMBER IT NAMES.** A fleet that has outrun the staged installer
        // and a fleet that is behind it are different claims, and `The release is 0.3.291` was
        // ambiguous enough to describe three machines that were on or past it (the thirty-third
        // list, measured on ux129).
        <p className="attention-release hint">
          {props.release.from === "installer"
            ? `The release is ${props.release.version}; the machines below are not on it.`
            : `The newest build in the fleet is ${props.release.version}; the machines below are behind it.`}
        </p>
      )}
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
                {/* **ONE VERB (audit-44 F9).** The attention row, the summary and the picker all
                    said `Open …`/`Use`; the visible verb is `Open` everywhere. The accessible name
                    still says which subject this door opens, because it opens Device → Machine
                    rather than the Overview the row/picker doors open. */}
                Open
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
 * **THE SELECTED MACHINE IS HIGHLIGHTED, NOT REPEATED.** The fleet row already states the name, the
 * reachability word, its age and the build, and the topbar's picker states the name and build for
 * the machine in force. The summary used to state the name, `Running`/`Offline` and a second age —
 * the same machine four times on one screen in three casings (audit-43 F2). It keeps only what the
 * row does not: what the machine can do (the live catalogue counts, or why they are absent) and
 * the two doors, Device → Overview and Connect a client. The row's `is-current` highlight is what
 * says which machine these facts belong to.
 */
function SelectedMachine(props: {
  device: Device;
  onTools: () => void;
  onOpenDevice: () => void;
  onConnect: () => void;
}) {
  const device = props.device;
  const label = device.name ?? shortId(device.id);
  // The live catalogue is a machine fact, not activity: it answers what this machine can do. The
  // hook is shared with the Overview tab, so the read is cached for both.
  const live = useLiveTools(device.id);
  const offline = !device.online || live.offline;
  const capabilities = new Set(live.tools.map((tool) => tool.capability)).size;
  const countsKnown = !offline && !(live.error && live.tools.length === 0);
  // **THE CARD NAMES WHAT IT IS ABOUT (audit-45 F1, revising audit-43 F2).** With 9VH selected the
  // card's whole text was `46 tools · 11 capabilities` and two doors: the machine's name and state
  // lived only in the topbar and in the highlighted row BELOW this card, so the card asked the
  // reader to remember the click. F2 read the row and the topbar as enough; the operator's own
  // brief (一目了然) is that a section headed "Selected machine" must say which one at a glance.
  // The line keeps F2's intent — the AGE stays the row's, and the version appears only when the
  // frame carried one — while stating the subject: name, the fleet's own presence word, version.
  // An offline machine keeps the full phrase with its last-seen age, because that age is what the
  // card's next sentence (counts unavailable) is about.
  // **THE HEADING IS THE SUBJECT; AN ONLINE MACHINE IS NOT ANNOUNCED TWICE (the thirty-fifth list).**
  // Measured on the live page with one machine selected: Home grew 908 → 952 px and a 154 px card
  // read `Selected machine · DESKTOP-9VH92BJ · online · 0.3.292 · 46 tools · 11 capabilities · Open ·
  // Connect a client` — while the topbar picker stated the name and build and the highlighted row
  // below stated the name, `online · seen Ns ago` and `v0.3.292`. Three of the card's four facts were
  // the page's own. The card now led by its label and then said the machine; it leads by the MACHINE
  // (the same one statement, in the place a section names its subject), and the identity line is gone
  // for an online machine. An OFFLINE machine keeps one line, because its last-seen age is the reason
  // the counts below are absent — that age is the only fact the row's `seen … ago` cannot give.
  return (
    <section className="card machine-summary" aria-label="Selected machine">
      <h2 className="machine-summary-name">{label}</h2>
      {!device.online && (
        <p className="machine-summary-who">
          <span className="machine-summary-presence">{presenceText(device)}</span>
        </p>
      )}
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
        <button
          type="button"
          className="primary"
          aria-label={`Open ${label}`}
          onClick={props.onOpenDevice}
        >
          Open
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
  const installerRead = useInstaller();
  // `null` for "no answer yet" is what `releaseTarget` already means by it, so the two non-answers
  // (still reading, failed) both pass `null` there — neither is a claim about the account.
  const installer =
    installerRead.state === "read" ? installerRead.installer : null;
  const fleet = props.fleetRead ?? { state: "ready" as const };
  // **THE PANEL MUST COME TO THE OPERATOR (audit-44 F7).** At 390 with a populated fleet the
  // enrollment panel opened at y=1045 with `scrollTop` still 0, so the press looked dead while the
  // code waited below the fold. The panel is scrolled into the viewport and its heading focused,
  // which is both the visual answer and where a keyboard reader resumes.
  const enrollTitle = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    if (!props.code) return;
    const title = enrollTitle.current;
    if (!title) return;
    if (typeof title.scrollIntoView === "function") title.scrollIntoView({ block: "start" });
    title.focus({ preventScroll: true });
  }, [props.code]);
  // **A ZERO NEEDS A READ BEHIND IT.** The empty fleet is a fact only when `/fleet` answered with
  // no machines; a page that is still reading, or whose read failed, holds an empty list too, and
  // telling that reader "No machines yet" invites enrollment for a fleet the console cannot see.
  const empty = fleet.state === "ready" && props.devices.length === 0;
  // The verdict and the attention rows are claims about the fleet now, so they wait for a
  // successful read. A failed read keeps the last list on screen and states the failure instead.
  const ready = fleet.state === "ready";
  const release = ready ? releaseTarget(props.devices, installer) : null;
  const problems = ready ? attentionProblems(props.devices, release) : [];
  // **THE BANNER NAMES THE FAILURE; THIS LINE ONLY SAYS WHAT IS MISSING.** When the read failed
  // there is no verdict to give, so the header draws none and the notice below states what the page
  // can show. A second failure sentence here made the page say one failure three times (audit-40
  // F4); the banner's title still carries the browser's cause.
  // **THE FLEET'S WATCHES, IN ONE LINE (the hundred-and-eightieth list).** The console can now start a watch on any machine, and
  // Home — the page that gives the fleet's verdict — said nothing about them. This reads each ONLINE machine's list once per
  // membership change (not per poll): four machines are four cheap calls, and a page that re-read them every few seconds would
  // spend more on this line than on the fleet itself.
  const [watches, setWatches] = useState<{ total: number; down: string[]; machines: number } | null>(null);
  const watchKey = props.devices.map((d) => `${d.id}:${d.online ? 1 : 0}`).join(",");
  const watchKeyRef = useRef("");
  useEffect(() => {
    const online = props.devices.filter((d) => d.online);
    if (online.length === 0) {
      setWatches(null);
      return;
    }
    if (watchKeyRef.current === watchKey) return;
    watchKeyRef.current = watchKey;
    let live = true;
    void Promise.all(
      online.map((d) =>
        readMonitors({ deviceId: d.id, envelope: null })
          .then((read) => ({ name: d.name ?? d.id, targets: read.targets }))
          .catch(() => null)
      )
    ).then((rows) => {
      if (!live) return;
      type Watched = { name: string; targets: MonitorTarget[] };
      const known = rows.filter((row): row is Watched => row !== null);
      const total = known.reduce((n, row) => n + row.targets.length, 0);
      const down = known.flatMap((row) =>
        row.targets.filter((t) => t.summary.upNow === false).map((t) => `${row.name}: ${t.id}`)
      );
      setWatches({ total, down, machines: known.length });
    });
    return () => {
      live = false;
    };
  }, [watchKey, props.devices]);

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
          {/* **ONE PAGE, ONE NAME (audit-45 F4).** The rail that opens this page says `Home`; the h1
              said `Machines`, and the tab title repeated it, so one destination had three names.
              The page is the fleet's verdict and its next action, not a table of machines, which is
              why it is named for the destination rather than for its rows. */}
          <h1>{PAGE_LABELS.home}</h1>
          {verdict !== null && (
            <p className={"home-verdict" + (problems.length > 0 ? " has-attention" : "")}>
              {verdict}
            </p>
          )}
          {watches !== null && watches.total > 0 && (
            <p className="hint home-services">
              {watches.down.length === 0
                ? `Watching ${watches.total} ${watches.total === 1 ? "service" : "services"} across ${watches.machines} ${watches.machines === 1 ? "machine" : "machines"} · all up`
                : `${watches.down.length} ${watches.down.length === 1 ? "service is" : "services are"} down: ${watches.down.join(", ")}`}
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
        // **THREE ROWS IS THE ATOM'S NUMBER, ON EVERY SURFACE THAT DRAWS IT (the eightieth list).** Measured
        // 2026-11-10: this page reserved TWO rows while the panel reserved three, and `layout-contract.json`
        // records `loading.rows: 3` — so the atom was describing one surface's habit. The page's own list
        // below draws a row per machine; the placeholder is the shape of what arrives, and three is the
        // number the contract names rather than the number this page happened to pick.
        <div className="skeleton-list" aria-label="Reading the machine list" role="status">
          {[0, 1, 2].map((row) => (
            <div className="skeleton-row" key={row}>
              <span className="skeleton skeleton-name" />
              <span className="skeleton skeleton-line" />
            </div>
          ))}
        </div>
      )}

      {fleet.state === "failed" && (
        // ONE FAILURE, ONE STATEMENT, ONE RETRY — ON THIS PAGE (audit-44 F3). The shell's banner
        // used to name the failure here too, so one failed read read as two failure blocks about
        // 70 px apart. This notice is now the one statement: it names the read in the console's
        // words, says what the list below still is, keeps the cause in its title, and owns the one
        // control that reads it again.
        <div className="notice" role="status">
          <span title={fleet.reason}>
            {props.devices.length > 0
              ? "The console could not read the machine list; the machines below are the last list it read."
              : "The console could not read the machine list."}
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
        <AttentionBlock
          problems={problems}
          fleet={fleet}
          release={release}
          onOpen={props.onOpenMachine}
        />
      )}

      {/* **THE MACHINE IN CONTEXT SITS BESIDE THE FLEET, NOT INSTEAD OF IT.** The verdict and the
          attention rows above are still the page's first answer; the selected row below is
          highlighted, and this section adds only the facts the row cannot: its live catalogue
          counts and its two doors. It is drawn whichever machine the fleet selected, including an
          offline one. */}
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
        <div className="card fleet-table-wrap">
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
        /* **A FIRST-RUN PAGE'S NEXT STEP IS THE INSTALLER, AND THE INSTALLER WAS ITS SMALLEST
           BLOCK AND ITS LAST (the fortieth list).** Measured on a fleet of zero at 1440: the page
           drew `page-head` 65 px, this empty state 100 px, and the installer strip 66 px BELOW
           both — the one thing a new operator can act on, drawn smallest and last, with a note
           above it saying "start with the installer below". And the strip's sentence on a fleet of
           zero was `Installer for a new or returning machine`, which describes a fleet that does
           not exist yet. So on a zero fleet the empty state and the strip are ONE block, in the
           order the work happens: what to do, then the door, then the file's own facts. */
        <section
          className="card first-run"
          aria-labelledby="first-run-title"
          data-testid="first-run"
        >
          {/* **THE HEADING NAMES THE THING THE READER CAN DO.** `Install the device runtime` is true
              only when the account HAS a file; when the release carries none, claiming "install"
              would be a heading for a door that is not there. */}
          <h2 id="first-run-title">
            {installerRead.state === "read" && installerRead.installer.available
              ? "Install the device runtime"
              : "Add your first machine"}
          </h2>
          <p className="hint">
            No machine has enrolled yet. Install this file on the machine you want to run, then
            press Add a machine for its enrollment command.
          </p>
          {installerRead.state === "read" ? (
            <InstallerBlock
              installer={installerRead.installer}
              devices={props.devices}
              onNotice={props.onNotice}
              bare
            />
          ) : installerRead.state === "failed" ? (
            <p className="hint installer-stale">
              The installer could not be read, so this page cannot offer the download or say which
              build it carries.{" "}
              <button type="button" className="mini" onClick={installerRead.retry}>
                Try the installer again
              </button>
            </p>
          ) : null}
        </section>
      ) : null}

      {/* The installer is the account's, not the read's: `/installer` is a separate answer, and it
          is the next step whether the machine list arrived, is still coming, or failed. It is a
          compact strip after the fleet, not the page's first card.
          **AND WHEN THE READ FAILS THE PAGE SAYS SO (the fortieth list).** The render used to be
          `{installer !== null && … }`, so a failed read drew nothing at all: measured live with
          `/installer` answering 500, Home lost 100 px and printed no sentence and no retry. The
          panel's Memory, Settings and Plugins pages already answer a failed read with their own
          subject and a Retry; this is the console's installer doing the same. */}
      {/* On a zero fleet the strip's job is the first-run card's (above); drawing both would be the
          page saying the same next step twice, which is the thing this list keeps finding. */}
      {empty ? null : installerRead.state === "read" ? (
        <InstallerBlock
          installer={installerRead.installer}
          devices={props.devices}
          onNotice={props.onNotice}
        />
      ) : installerRead.state === "failed" ? (
        <section className="card installer installer-strip" aria-label="Install the device runtime">
          <h2>Install the device runtime</h2>
          <p className="hint installer-stale">
            The installer could not be read, so this page cannot offer the download or say which
            build it carries.
          </p>
          <button type="button" className="mini" onClick={installerRead.retry}>
            Try the installer again
          </button>
        </section>
      ) : null}

      {props.code && (
        <section className="enroll" aria-labelledby="enroll-title">
          <h2 id="enroll-title" ref={enrollTitle} tabIndex={-1}>
            Enrollment command
          </h2>
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
