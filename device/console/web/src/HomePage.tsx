import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { LiveNow } from "./LiveNow";
import type { LiveLine, StartedCall } from "./liveActivity";
import { api, type AuditLine, type Device, type Installer } from "./api";
import { AttemptReason, attemptCount, attemptReason } from "./attempt";
import { DeviceCard } from "./DeviceCard";
import { copyText } from "./copy";
import { clockTime, coverageClause, shortId } from "./format";
import { bytes } from "./runFormat";
import { listPlugins } from "./pluginApi";
import { surfaceHash } from "./surface-link";
import { useLiveTools } from "./useLiveTools";
import { ActivityTimeline } from "./ActivityTimeline";
import {
  ACTIVITY_LIMIT,
  capActivity,
  hubWindowSentence,
  mergeActivityReport
} from "./activityMerge";
import {
  isChannelBoundary,
  isClientCall,
  isConsoleRead,
  isPoll,
  isUnattributed
} from "./activitySummary";
import { auditFailed } from "./auditFilter";

/** A one-time enrollment code and the command that redeems it. */
export type Enrollment = { code: string; expires_at_ms: number } | null;

type FleetProps = {
  devices: Device[];
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
 * The installer the console was given, read once when the fleet is drawn.
 *
 * `null` is "nothing to show": the read has not landed, or it failed. A failed read hides the block
 * rather than drawing a door that cannot open, and the page does not poll it — the next visit to
 * the fleet is the next read.
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
 * Where to get the device runtime, above the command that assumes it is installed.
 *
 * Every value is the `/installer` answer: the filename and version are the console's staged file,
 * the size and short hash are over the bytes the link serves. An unavailable installer is one
 * sentence and no link — the page offers no download it cannot honour.
 */
function InstallerBlock(props: { installer: Installer }) {
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
    <section className="installer" aria-label="Install the device runtime">
      <h2>Install the device runtime</h2>
      <a className="btn primary" href="/installer/download" download>
        Download Windows installer
      </a>
      <p className="hint">
        {installer.filename} · {bytes(installer.size_bytes)} · sha256{" "}
        {installer.sha256.slice(0, 8)}…
      </p>
    </section>
  );
}

/**
 * Home before a machine is selected: the fleet. One row per machine, one accent for the action that
 * adds one, and the enrollment command when it exists. This is the previous Devices page kept because
 * it is the only way in — restyled to the hub's shell, not given a second dashboard.
 */
function FleetHome(props: FleetProps) {
  const online = props.devices.filter((device) => device.online).length;
  const installer = useInstaller();
  return (
    <div className="page home-page">
      <header className="page-head">
        <div>
          <h1>Machines</h1>
          <p className="page-sub">
            {props.devices.length} machine{props.devices.length === 1 ? "" : "s"} · {online} online
          </p>
        </div>
        <div className="page-actions">
          <button type="button" onClick={props.onTokens}>
            Tokens
          </button>
          <button type="button" className="primary" disabled={props.busy} onClick={props.onAddMachine}>
            Add a machine
          </button>
        </div>
      </header>

      {installer !== null && <InstallerBlock installer={installer} />}

      {props.code && (
        <section className="enroll" aria-label="Enrollment command">
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

      {props.devices.length === 0 ? (
        <section className="empty-state">
          <h2>No machines yet</h2>
          <p className="hint">
            A machine appears here after it enrolls with this console. Run the command once on the
            Windows machine, and it dials out to this console.
          </p>
        </section>
      ) : (
        <div className="fleet-list">
          {props.devices.map((device) => (
            <DeviceCard
              key={device.id}
              device={device}
              selected={props.selectedId === device.id}
              // The staged installer's version is a fact of the same page: Add-a-machine offers
              // that file, and each row says when the machine it opens is older than it.
              installerVersion={installer?.available ? installer.version : null}
              onSelect={() => props.onOpen(device.id)}
              onRename={() => props.onRename(device)}
              onRemove={() => props.onRemove(device)}
              onDiagnostics={() => props.onDiagnostics(device)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

type PluginCount = number | null;

/// How many plugins the selected machine hosts. `known` is false while the tool list is unread or
/// unreadable; an older build has no `plugins.list` at all and counts 0, which is the truth for it.
function usePluginCount(deviceId: string | null, known: boolean, present: boolean): PluginCount {
  const [count, setCount] = useState<PluginCount>(null);
  useEffect(() => {
    if (deviceId === null || !known) {
      setCount(null);
      return;
    }
    if (!present) {
      setCount(0);
      return;
    }
    let live = true;
    setCount(null);
    listPlugins(deviceId)
      .then((plugins) => {
        if (live) setCount(plugins.length);
      })
      .catch(() => {
        // A plugin read that fails leaves the count unknown; Home does not invent a zero.
        if (live) setCount(null);
      });
    return () => {
      live = false;
    };
  }, [deviceId, known, present]);
  return count;
}

function plural(count: number, noun: string, many = `${noun}s`): string {
  return `${count} ${count === 1 ? noun : many}`;
}

/** Midnight in the console's own timezone; the hub's timeline is today's, not the audit window's. */
function startOfToday(now = Date.now()): number {
  const date = new Date(now);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

/** Whether two instants fall on the same local day: the midnight rule, said once. */
function sameDay(a: number, b: number): boolean {
  const first = new Date(a);
  const second = new Date(b);
  return (
    first.getFullYear() === second.getFullYear() &&
    first.getMonth() === second.getMonth() &&
    first.getDate() === second.getDate()
  );
}

/** The day an older line belongs to, named only when the summary can keep the word. */
function dayWord(ts: number, now: number): string {
  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  if (sameDay(ts, yesterday.getTime())) return " yesterday";
  const date = new Date(ts);
  return ` on ${date.getMonth() + 1}/${date.getDate()}`;
}

/**
 * How many rows of the day the hub draws before folding the rest behind one control.
 *
 * The hub's timeline is one machine's whole day: on the busiest machine measured (2026-11-05) that
 * was 935 rows, a 51,620 px page with the capability line 56 screens down. Sixty rows is a day a
 * person can scan; the remaining rows are still one click away, in place, never paginated out of
 * reach. The Activity page keeps the whole record and is not folded.
 */
const HUB_DAY_ROWS = 60;

/**
 * The hub's one line about the day.
 *
 * **THE COUNT IS THE DAY'S, THE "LAST" IS THE WINDOW'S.** The count and the failures come from
 * `/fleet`'s `today`, which is the local calendar day (the same midnight the timeline cuts at); the
 * fetched window only says when the machine last worked, because the newest client call is always
 * the newest line and the window holds the newest lines. When the day read cannot prove the whole
 * day, it says `since` — a rotation or the scan bound is stated, not hidden.
 *
 * Only a client's calls count: the console's own failed polls are in the same audit and are not the
 * AI's work. A window with no line today still knows when the machine last worked, so it says that
 * instead of letting the empty timeline above read as a machine that did nothing overnight. A cut or
 * stale window is already named by the cap and read sentences; this line never repeats those.
 */
function todayLine(
  day: Device["today"],
  lines: readonly AuditLine[],
  now = Date.now()
): string {
  const calls = lines.filter(isClientCall);
  const start = startOfToday(now);
  const today = calls.filter((line) => line.ts_ms >= start);
  const newestToday = today.reduce<AuditLine | null>(
    (latest, line) => (latest === null || line.ts_ms > latest.ts_ms ? line : latest),
    null
  );
  if (day && day.client_calls > 0) {
    // The count is the day's; the moment is the newest call the window holds. In the thousands-of-
    // console-reads case the window may hold no client call at all, and then the line states the
    // count without inventing a time it does not have.
    return (
      `Today ${plural(day.client_calls, "client call")}` +
      (day.failed > 0 ? ` · ${day.failed} failed` : "") +
      (newestToday !== null ? ` · last ${clockTime(newestToday.ts_ms)}` : "") +
      coverageClause(day)
    );
  }
  if (day) {
    const newest = calls.reduce<AuditLine | null>(
      (latest, line) => (latest === null || line.ts_ms > latest.ts_ms ? line : latest),
      null
    );
    const base = newest === null
      ? "Today no client calls"
      : `Today no client calls · last ${clockTime(newest.ts_ms)}${dayWord(newest.ts_ms, now)}`;
    return base + coverageClause(day);
  }
  // No day read: the fleet has not answered yet (or the read failed). The window is the fallback
  // while it is absent; a call today would make the fleet's day read name the machine, so this
  // branch can only undercount a transiently unavailable day, never replace one.
  if (today.length === 0) {
    const newest = calls.reduce<AuditLine | null>(
      (latest, line) => (latest === null || line.ts_ms > latest.ts_ms ? line : latest),
      null
    );
    if (newest === null) return "Today no client calls";
    return `Today no client calls · last ${clockTime(newest.ts_ms)}${dayWord(newest.ts_ms, now)}`;
  }
  const newest = today.reduce((latest, line) => Math.max(latest, line.ts_ms), 0);
  const failed = today.filter(auditFailed).length;
  return (
    `Today ${plural(today.length, "client call")}` +
    (failed > 0 ? ` · ${failed} failed` : "") +
    ` · last ${clockTime(newest)}`
  );
}

/**
 * How long a window may lag the live lines before the page says when it was read.
 *
 * The app's poll is five seconds and a reread needs a round trip. Under that grace the tick that
 * would reread the window is still on its way, and a sentence that appeared and vanished on every
 * live line would be noise, not a fact. Past it, the window has missed its reread and the page says
 * how old it is.
 */
const READ_GRACE_MS = 6_000;

/** "Read 2 min ago." — the one sentence under a window that is older than the live lines. */
function readAgo(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  if (seconds < 60) return `Read ${seconds} s ago.`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `Read ${minutes} min ago.`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `Read ${hours} h ago.`;
  return `Read ${Math.floor(hours / 24)} d ago.`;
}

/**
 * When the fetched window is behind the live lines, say when it was read.
 *
 * The timeline merges the two, so a reader cannot tell the fetched part's age from the rows: the
 * live lines are newer than the window's newest line, and the window below them may be hours old.
 * The sentence redraws on its own second so an unanswered reread keeps counting up; it disappears
 * as soon as the next read brings a window that includes the live line.
 */
function WindowRead(props: { readAt: number | null; fetchedNewest: number; liveNewest: number }) {
  const [, setTick] = useState(0);
  const behind =
    props.readAt !== null &&
    props.liveNewest > 0 &&
    (props.fetchedNewest === 0 || props.liveNewest > props.fetchedNewest);
  useEffect(() => {
    if (!behind) return;
    const timer = window.setInterval(() => setTick((current) => current + 1), 1000);
    return () => window.clearInterval(timer);
  }, [behind]);
  if (props.readAt === null || !behind) return null;
  const elapsed = Date.now() - props.readAt;
  if (elapsed < READ_GRACE_MS) return null;
  // It counts up every second, so it is not a live region: a status role would re-announce the
  // same fact once a second. The note is read when the page is read.
  return <p className="hint home-read">{readAgo(elapsed)}</p>;
}

/**
 * The offline hub's one recovery sentence, with its two doors named by where they work.
 *
 * `Retry` is the console's own attempt at the stream. The Device → Machine view is the console's
 * read of the machine's last facts, and its address is the surface fragment every other navigation
 * uses. The device's panel listens on that machine's loopback only, so its URL is text — a place on
 * the machine, not a link this console can promise to open.
 */
function OfflineAdvice() {
  return (
    <>
      The console keeps trying; Retry works from here. If it stays offline, open{" "}
      <a href={surfaceHash({ page: "device", tab: "machine" })}>Device → Machine</a> in the console
      for the machine's last facts. The machine's local panel is{" "}
      <code className="mono">http://127.0.0.1:18090/</code> — it opens only at the machine.
    </>
  );
}

/**
 * The hub, once a machine is chosen: is it running, what is it doing now, what did it do today.
 * One status line, one Connect action, the Now band, this machine's timeline and one counts line.
 * Identity is the header's; this page does not repeat it, and the catalogue is the Tools tab's job,
 * not a tile grid. The console's endpoint lives in Connect, so this page does not name it; the
 * offline advice names the machine's own panel as a place on the machine, not a door from here.
 */
function HubHome(
  props: FleetProps & {
    device: Device;
    onConnect: () => void;
    onTools: () => void;
    onActivity: () => void;
    /** The live activity stream, newest first. Absent in tests and before the stream opens. */
    liveLines?: LiveLine[];
    /** The calls the proxy has announced and not cleared, for this machine or another. */
    liveRunning?: StartedCall[];
    liveState?: "connecting" | "live" | "offline";
    /** The app's poll count: each new value is one tick, one chance to reread this machine's day. */
    tick?: number;
    /** How many snapshots have arrived after the stream dropped; each is one reread. */
    liveRecoveries?: number;
    /** Open a machine's Terminal tab, where an AI's session can be watched. */
    onWatchTerminal?: (deviceId: string) => void;
  }
) {
  const device = props.device;
  const name = device.name ?? shortId(device.id);
  const live = useLiveTools(device.id);
  // A stale catalogue still names the tool surface, so the plugin read is allowed; an empty list
  // after a failed read knows nothing and must not be read as "an older build".
  const known = live.tools.length > 0 || (!live.loading && live.error === null);
  const pluginsTool = live.tools.some((tool) => tool.name === "plugins.list");
  const plugins = usePluginCount(device.id, known, pluginsTool);
  // The page's subject is the machine: the stream's lines are the newest of the same audit, and
  // both the Now band and the timeline drop every other machine's. The timeline is today's; older
  // days belong to Activity, behind the All activity link.
  const liveLines = (props.liveLines ?? []).filter((line) => line.device_id === device.id);
  // The band is this machine's, like the timeline: another machine's in-flight call must not name
  // itself on this page. The caller filter lives in the band (it counts the AI, not the console).
  const liveRunning = (props.liveRunning ?? []).filter(
    (call) => call.device_id === device.id
  );
  // THE HUB READS ITS OWN MACHINE. Filtering the fleet window here let another machine's newer
  // traffic push this machine's day out, and then the empty sentence below claimed there had been
  // no calls at all. `/audit?device=` applies the filter during the server's scan, so the window is
  // this machine's 1000 lines whatever the rest of the fleet is doing.
  const [machineAudit, setMachineAudit] = useState<{
    device: string;
    lines: AuditLine[];
    /** When the answer landed: the window is as fresh as this moment, and no fresher. */
    readAt: number;
  } | null>(null);
  const [auditError, setAuditError] = useState(false);
  // The hub draws the newest rows of the day; this remembers that the rest were asked for. It
  // resets with the machine, so one machine's expanded day cannot stand for another's.
  const [wholeDay, setWholeDay] = useState(false);
  // **THE HUB IS THE AI'S WORK.** The console's own calls (and the failed polls it no longer
  // records as work) are in the machine's audit and are not the thing this page is about, so they
  // are folded behind one counted chip and drawn only when the reader asks. The state is this
  // view's, not a stored preference: another machine starts folded.
  const [showConsoleReads, setShowConsoleReads] = useState(false);
  // The third fold: the day's lines that belong to neither the AI nor the console. They are real
  // rows whose caller was never recorded; Activity has always counted them and the hub used to
  // leave them out of every control. Also this view's, reset with the machine.
  const [showUnattributed, setShowUnattributed] = useState(false);
  useEffect(() => {
    setWholeDay(false);
    setShowConsoleReads(false);
    setShowUnattributed(false);
  }, [device.id]);

  // A read that answers after the page switched machines is another machine's answer, and must not
  // replace this one's window.
  const showing = useRef(device.id);
  showing.current = device.id;
  // At most one read per machine. The tick and a reconnect can arrive together; the first starts
  // the request and the second is dropped rather than stacked on top of it. The next tick reads
  // again, so a dropped trigger costs freshness, never correctness.
  const reading = useRef<string | null>(null);
  const readStarted = useRef(0);
  const readApplied = useRef(0);
  const readMachine = useCallback(async () => {
    if (reading.current === device.id) return;
    const generation = ++readStarted.current;
    reading.current = device.id;
    try {
      const lines = await api.audit(ACTIVITY_LIMIT, device.id);
      if (showing.current === device.id && generation > readApplied.current) {
        readApplied.current = generation;
        setMachineAudit({ device: device.id, lines, readAt: Date.now() });
        setAuditError(false);
      }
    } catch {
      // A failed read keeps the last window on screen and says it could not be read; it never
      // counts as an answer that supersedes one that did arrive.
      if (showing.current === device.id && generation > readApplied.current) {
        setAuditError(true);
      }
    } finally {
      if (reading.current === device.id) reading.current = null;
    }
  }, [device.id]);

  // The first read, and a new one when the machine changes.
  useEffect(() => {
    setMachineAudit(null);
    setAuditError(false);
    void readMachine();
  }, [readMachine]);

  // The app's five-second poll is this page's clock: each tick rereads the day rather than letting
  // the one mount-time window stand for it after a hiccup.
  const tick = props.tick;
  const seenTick = useRef(tick);
  useEffect(() => {
    if (tick === undefined || tick === seenTick.current) return;
    seenTick.current = tick;
    void readMachine();
  }, [tick, readMachine]);

  // A snapshot after a dropped connection replaced the live list. Whatever was written while the
  // stream was down is a hole between this fetched window and the snapshot's oldest line; reread
  // the window once, so the day is whole again.
  const recoveries = props.liveRecoveries;
  const seenRecovery = useRef(recoveries);
  useEffect(() => {
    if (recoveries === undefined || recoveries === seenRecovery.current) return;
    seenRecovery.current = recoveries;
    void readMachine();
  }, [recoveries, readMachine]);

  const now = Date.now();
  const todayStart = startOfToday(now);
  const loaded = machineAudit?.device === device.id;
  const fetched = loaded && machineAudit ? machineAudit.lines : [];
  // The merged window keeps the older lines the timeline hides: the summary line below needs one of
  // them to say when the machine last worked. The timeline itself is still today's only. The report
  // also carries the repeats the merge dropped, which the cap sentence names.
  const mergedWindow = mergeActivityReport(fetched, liveLines);
  const windowLines = mergedWindow.lines.sort((a, b) => b.ts_ms - a.ts_ms);
  // A channel boundary is in the audit to prove an absence, not to be drawn as the machine's work.
  const windowBoundaries = windowLines.filter(isChannelBoundary).length;
  const activityLines = windowLines.filter((line) => !isChannelBoundary(line));
  const today = capActivity(activityLines.filter((line) => line.ts_ms >= todayStart));
  // **THE HUB'S WINDOW HAS THREE DRAWABLE BUCKETS AND THREE WITHHELD KINDS.** One rule, Activity's
  // header's: every non-boundary line is a client call (`isClientCall`), the console's own read
  // (`isConsoleRead`), a failed poll (`isPoll`), or unattributed (`isUnattributed` — a real line
  // with no recorded caller and no token to prove one). The hub draws the first three; the fold and
  // its two chips hold exactly those rows. The measured 2026-11-05 hub had 1004 fetched lines under
  // a sentence that named 974, while expanding and turning on the console chip drew 835: the missing
  // rows were unattributed lines and polls, which no control offered. The sentence's total is now
  // the three buckets, and every number in it is one of their counts or a withheld count no larger
  // than their sum; polls, boundaries and repeats are named as not drawn.
  const clientRows = today.lines.filter(isClientCall);
  const consoleRows = today.lines.filter(isConsoleRead);
  const pollRows = today.lines.filter(isPoll);
  const unattributedRows = today.lines.filter(isUnattributed);
  const visibleRows = today.lines.filter(
    (line) =>
      isClientCall(line) ||
      (showConsoleReads && isConsoleRead(line)) ||
      (showUnattributed && isUnattributed(line))
  );
  // **THE DAY IS SCANNABLE; THE RECORD IS NOT HIDDEN.** The newest rows are drawn and one control
  // offers the rest of today in place. The count is the drawn list's own length minus what is
  // drawn, so a live frame that arrives while the day is folded grows the label by the same one
  // row it adds — and the count follows the chips, because a chip changes the list.
  const hiddenDayRows = visibleRows.length - HUB_DAY_ROWS;
  const dayRows = wholeDay ? visibleRows : visibleRows.slice(0, HUB_DAY_ROWS);
  const hubCounts = {
    client: clientRows.length,
    console: consoleRows.length,
    unattributed: unattributedRows.length,
    polls: pollRows.length,
    boundaries: windowBoundaries,
    repeats: mergedWindow.repeats,
    shown: dayRows.length
  };
  // Before the first window arrives there is nothing to count; the timeline says it is reading. Once
  // a window (or a live line) is in hand, the summary is a fact about the lines on the page.
  const summary =
    loaded || liveLines.length > 0 ? todayLine(device.today, windowLines, now) : null;
  // The window's own newest line and the stream's. When the stream holds a newer line than the
  // window does, the fetched part is behind the live part and the page says when it was read.
  const fetchedNewest = fetched.reduce((newest, line) => Math.max(newest, line.ts_ms), 0);
  const liveNewest = liveLines.reduce((newest, line) => Math.max(newest, line.ts_ms), 0);
  const readAt = loaded && machineAudit ? machineAudit.readAt : null;
  // The server returns this machine's newest 1000 lines. If the oldest of them is still today then
  // today's list may continue behind that window; if it is yesterday, every today line is already
  // here. Live lines can also push the merged list past the cap.
  const oldestFetched = fetched[fetched.length - 1];
  const ownWindowCut =
    today.capped ||
    (fetched.length >= ACTIVITY_LIMIT &&
      oldestFetched !== undefined &&
      oldestFetched.ts_ms >= todayStart);
  // The hub's own sentence, not Activity's cap sentence: its controls hold a different set from
  // the fetched window, and the sentence counts exactly what they hold. It is drawn where Activity
  // draws its cap sentence, and only when this machine's window was cut.
  const windowSentence = ownWindowCut ? hubWindowSentence(hubCounts) : null;
  // **THE REPEATS ARE NAMED WHEREVER THE WINDOW DROPS THEM, CUT OR NOT.** The day line is `/fleet`'s
  // day read, which counts every line the file holds; the fold draws the merged window, which draws
  // a repeated `(ts_ms, tool, status)` key once. On 2026-11-05 that was 183 counted over 179
  // drawable, and an uncut window had no sentence at all, so the four rows vanished silently. This
  // is Activity's cap-sentence fact, in its words, beside the fold. A cut window already prints it
  // in `hubWindowSentence`, so the two cannot say the same count twice.
  const repeatsSentence =
    mergedWindow.repeats > 0 && windowSentence === null
      ? `${plural(mergedWindow.repeats, "repeat")} not drawn.`
      : null;
  // The empty sentence names what the list is missing rather than claiming nothing happened. With
  // console reads folded, an empty list is usually the console's own polling, and the chip above is
  // the way to it; a window with only unattributed rows is not the AI's work either, and says so
  // without pretending the console's reads are hidden when there are none.
  const empty = auditError
    ? "The console could not read this machine's activity, so this list may be incomplete."
    : !loaded
      ? "Reading this machine's activity…"
      : consoleRows.length > 0 && !showConsoleReads
        ? "No client calls yet; the console's own reads are hidden."
        : today.lines.length > 0
          ? showConsoleReads
            ? `No client calls or console reads from ${name} today.`
            : `No client calls from ${name} today.`
          : `No calls from ${name} today. When an AI client calls this machine with its token, ` +
            "its work appears here; Connect a client sets one up.";

  const offline = !device.online || live.offline;
  // **ONE PLACE FOR EACH BUILD.** The subtitle used to be "Running · console v0.3.273-ux43", while
  // the Account menu already prints "Console 0.3.273-ux43"; at 390 the header chip drops the
  // device's own "v0.3.274", which made the console's the page's only build — the menu's fact said
  // twice. The subtitle says what the machine is; the chip and the menu own the versions.
  const statusLine = offline ? "Offline" : "Running";
  const capabilities = new Set(live.tools.map((tool) => tool.capability)).size;
  // THE FAILURE SENTENCE SAYS WHY. "Not connected" alone leaves the operator choosing between the
  // edge and the machine; the attempt count and the device's own reason are that choice. The reason
  // is cut to one line and titled, because a phone-width banner has room for the count but not for a
  // 51-character channel sentence. A device that gave no reason keeps the sentence it had.
  const failedReason = attemptReason(device.last_attempt);
  const failedAttempts = device.last_attempt?.attempts ?? 0;
  const notice: ReactNode = offline
    ? failedReason !== ""
      ? (
          <>
            {name} is not connected after {attemptCount(failedAttempts)} —{" "}
            <AttemptReason reason={failedReason} />. <OfflineAdvice />
          </>
        )
      : (
          <>
            {name} is not connected. <OfflineAdvice />
          </>
        )
    : live.error
      ? "The machine is running. The console could not read its tool list, so these counts may be stale."
      : null;

  return (
    <div className="page home-page">
      <header className="page-head">
        <div>
          <h1>Home</h1>
          <p className="page-sub">{statusLine}</p>
        </div>
        <div className="page-actions">
          <button type="button" onClick={props.onConnect}>
            Connect a client
          </button>
        </div>
      </header>

      {notice && (
        <div className="notice" role="status">
          <span>{notice}</span>
          <button type="button" onClick={() => live.retry()}>
            Retry
          </button>
        </div>
      )}

      <LiveNow
        // The band and the timeline are the same machine's day: feeding the band the fleet's
        // newest 200 live lines made it say "Quiet" while the window below held this machine's
        // calls, and after a reconnect the live list is only the newest forty.
        lines={today.lines}
        running={liveRunning}
        devices={props.devices}
        state={props.liveState ?? "connecting"}
        onAll={props.onActivity}
        onWatch={props.onWatchTerminal}
      />

      {/* **THE CAPABILITY LINE IS ABOVE THE LOG.** What this machine can do was drawn after the
          timeline, which on the busiest measured machine put it 51,599 px down a 51,620 px page.
          The line is the answer to a first question, so it sits with the Now band, not after the
          day's record. */}
      {offline ? (
        <p className="hint">Tool counts are unavailable while the machine is offline.</p>
      ) : live.error && live.tools.length === 0 ? (
        <p className="hint">Tool counts are unavailable until the console can read the machine's list.</p>
      ) : (
        <button type="button" className="counts-line" onClick={props.onTools}>
          {plural(live.tools.length, "tool")} · {plural(capabilities, "capability", "capabilities")}
          {plugins !== null ? ` · ${plural(plugins, "plugin")}` : ""}
          {live.stale ? " · stale" : ""} <span aria-hidden="true">→</span>
        </button>
      )}

      <section className="panel home-timeline" aria-label="This machine's activity">
        {/* One line, above the rows: the day's count from `/fleet`'s day read, and the window's
            newest call for "last". The rows below are still the fetched window, and the cap
            sentence names it when they are cut. It is not a live region: it moves on the
            five-second reread, not on every line, and a status role would re-announce the same
            count. */}
        {summary !== null && <p className="home-today">{summary}</p>}
        {auditError && today.lines.length > 0 && (
          // Live lines are on screen, but the history behind them is unread: the page says which
          // half it is showing instead of letting the list stand for the whole day.
          <p className="hint" role="status">
            The console could not read this machine's audit, so this timeline shows only calls that
            arrive live.
          </p>
        )}
        <ActivityTimeline
          lines={dayRows}
          devices={props.devices}
          capped={ownWindowCut}
          windowSentence={windowSentence}
          // The hub's timeline is already this machine's day: a machine chip here could only say
          // "All machines" over one machine's lines, which would be a lie.
          showFilters={false}
          // The console's own reads are counted here, folded out of the list until asked for. The
          // count is the day's console rows, not the rows the small fold happens to draw: the chip
          // must name everything one click adds.
          consoleReads={
            loaded
              ? {
                  count: consoleRows.length,
                  shown: showConsoleReads,
                  onToggle: () => setShowConsoleReads((shown) => !shown)
                }
              : undefined
          }
          // The third fold names the rows no caller was recorded for. Activity's header counts the
          // same bucket; the hub now offers the same rows instead of leaving them out of every list
          // and every number. It is drawn only when it has rows.
          unattributed={
            loaded
              ? {
                  count: unattributedRows.length,
                  shown: showUnattributed,
                  onToggle: () => setShowUnattributed((shown) => !shown)
                }
              : undefined
          }
          onWatch={props.onWatchTerminal}
          empty={empty}
        />
        {!wholeDay && hiddenDayRows > 0 && (
          // A real control over the same list: it expands in place, so the count it shows is the
          // rows behind it, not a page number made up from a cut. A live frame that lands while
          // folded redraws this label with one more, because it is derived, not remembered.
          <button type="button" className="link home-more" onClick={() => setWholeDay(true)}>
            Show the rest of today ({hiddenDayRows} more)
          </button>
        )}
        {repeatsSentence !== null && (
          // The day line counts the day; the fold draws the merge. This is the row difference
          // Activity's cap sentence names, so 60 + 119 + 4 = 183 has a sentence that says so.
          <p className="hint home-repeats">{repeatsSentence}</p>
        )}
        <WindowRead readAt={readAt} fetchedNewest={fetchedNewest} liveNewest={liveNewest} />
      </section>
    </div>
  );
}

export type HomePageProps = FleetProps & {
  /// The machine in context. Null is the fleet zero state; a chosen machine is the hub.
  device: Device | null;
  onConnect: () => void;
  onTools: () => void;
  onActivity: () => void;
  /** The live activity stream, newest first. Absent in tests and before the stream opens. */
  liveLines?: LiveLine[];
  /** The calls the proxy has announced and not cleared, for any machine. */
  liveRunning?: StartedCall[];
  liveState?: "connecting" | "live" | "offline";
  /** The app's poll count: each new value is one tick, one chance to reread this machine's day. */
  tick?: number;
  /** How many snapshots have arrived after the stream dropped; each is one reread. */
  liveRecoveries?: number;
  /** Open a machine's Terminal tab, where an AI's session can be watched. */
  onWatchTerminal?: (deviceId: string) => void;
};

export function HomePage(props: HomePageProps) {
  return props.device ? <HubHome {...props} device={props.device} /> : <FleetHome {...props} />;
}
