import { useCallback, useEffect, useRef, useState } from "react";
import { LiveNow } from "./LiveNow";
import type { LiveLine, StartedCall } from "./liveActivity";
import { api, type AuditLine, type Device } from "./api";
import { DeviceCard } from "./DeviceCard";
import { copyText } from "./copy";
import { shortId } from "./format";
import { listPlugins } from "./pluginApi";
import { useLiveTools } from "./useLiveTools";
import { ActivityTimeline } from "./ActivityTimeline";
import { ACTIVITY_LIMIT, capActivity, mergeActivity } from "./activityMerge";
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
  onDismissCode: () => void;
  onNotice: (sentence: string) => void;
};

/**
 * Home before a machine is selected: the fleet. One row per machine, one accent for the action that
 * adds one, and the enrollment command when it exists. This is the previous Devices page kept because
 * it is the only way in — restyled to the hub's shell, not given a second dashboard.
 */
function FleetHome(props: FleetProps) {
  const online = props.devices.filter((device) => device.online).length;
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
          <button type="button" className="primary" disabled={props.busy} onClick={props.onAddMachine}>
            Add a machine
          </button>
        </div>
      </header>

      {props.code && (
        <section className="enroll" aria-label="Enrollment command">
          <p className="hint">
            This code works once, for 15 minutes. On the Windows machine, run this in an elevated
            Command Prompt, then restart the <code>SummriseLaunch</code> service.
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

/** A clock a person reads on the summary line: HH:MM in the console's own timezone. */
function shortClock(ts: number): string {
  const date = new Date(ts);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
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
 * The hub's one line about the day, computed from the lines already in hand.
 *
 * "Today" is local midnight — the same midnight the timeline cuts at — and only a client's calls
 * count: the console's own polling is in the same audit and is not the AI's work. A window with no
 * line today still knows when the machine last worked, so it says that instead of letting the empty
 * timeline above read as a machine that did nothing overnight. A cut or stale window is already
 * named by the cap and read sentences; this line never repeats those.
 */
/**
 * Whether a line is a client's call.
 *
 * The `caller` field is newer than the audit itself: 908 of the live 1000 lines have no caller, and
 * 376 of those carry a token — no console line carries one (0 of 30). Counting only `caller ===
 * "client"` therefore undercounted the busiest machine's day by 5.9× (62 counted, 363 proven). A line
 * with a token is a client call even when the caller was never recorded; a console line never is.
 */
function isClientCall(line: AuditLine): boolean {
  if (line.caller === "client") return true;
  if (line.caller === "console") return false;
  return line.token_id != null;
}

function todayLine(lines: readonly AuditLine[], now = Date.now()): string {
  const calls = lines.filter(isClientCall);
  const start = startOfToday(now);
  const today = calls.filter((line) => line.ts_ms >= start);
  if (today.length === 0) {
    const newest = calls.reduce<AuditLine | null>(
      (latest, line) => (latest === null || line.ts_ms > latest.ts_ms ? line : latest),
      null
    );
    if (newest === null) return "Today no client calls";
    return `Today no client calls · last ${shortClock(newest.ts_ms)}${dayWord(newest.ts_ms, now)}`;
  }
  const newest = today.reduce((latest, line) => Math.max(latest, line.ts_ms), 0);
  const failed = today.filter(auditFailed).length;
  return (
    `Today ${plural(today.length, "client call")}` +
    (failed > 0 ? ` · ${failed} failed` : "") +
    ` · last ${shortClock(newest)}`
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
 * The hub, once a machine is chosen: is it running, what is it doing now, what did it do today.
 * One status line, one Connect action, the Now band, this machine's timeline and one counts line.
 * Identity is the header's; this page does not repeat it, and the catalogue is the Tools tab's job,
 * not a tile grid. The endpoint lives in Connect, so this page never names the URL.
 */
function HubHome(
  props: FleetProps & {
    device: Device;
    onConnect: () => void;
    onTools: () => void;
    onActivity: () => void;
    /** The console's own build, from /health; the chip next door is the device's. */
    consoleVersion?: string | null;
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
  // them to say when the machine last worked. The timeline itself is still today's only.
  const windowLines = mergeActivity(fetched, liveLines).sort((a, b) => b.ts_ms - a.ts_ms);
  const timeline = capActivity(windowLines.filter((line) => line.ts_ms >= todayStart));
  // Before the first window arrives there is nothing to count; the timeline says it is reading. Once
  // a window (or a live line) is in hand, the summary is a fact about the lines on the page.
  const summary = loaded || liveLines.length > 0 ? todayLine(windowLines, now) : null;
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
    timeline.capped ||
    (fetched.length >= ACTIVITY_LIMIT &&
      oldestFetched !== undefined &&
      oldestFetched.ts_ms >= todayStart);
  const empty = auditError
    ? "The console could not read this machine's activity, so this list may be incomplete."
    : !loaded
      ? "Reading this machine's activity…"
      : `No calls from ${name} today. When an AI client calls this machine with its token, ` +
        "its work appears here; Connect a client sets one up.";

  const offline = !device.online || live.offline;
  // **THE MACHINE CHIP ALREADY SAYS "Running · v…".** Repeating it here spent the page's one subtitle
  // line on a fact the header had just stated; the console's own build is the fact the chip cannot
  // carry, and the one a person needs when a page and a device disagree about what is deployed.
  const statusLine = [
    offline ? "Offline" : "Running",
    props.consoleVersion ? `console v${props.consoleVersion}` : null
  ]
    .filter(Boolean)
    .join(" · ");
  const capabilities = new Set(live.tools.map((tool) => tool.capability)).size;
  const notice = offline
    ? `${name} is not connected. The console keeps trying; open the machine's panel if it stays offline.`
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
        lines={timeline.lines}
        running={liveRunning}
        devices={props.devices}
        state={props.liveState ?? "connecting"}
        onAll={props.onActivity}
        onWatch={props.onWatchTerminal}
      />

      <section className="panel home-timeline" aria-label="This machine's activity">
        {/* One line, above the rows it counts: the day at a glance, from the same window the
            timeline draws. It is not a live region: it moves on the five-second reread, not on every
            line, and a status role would re-announce the same count. */}
        {summary !== null && <p className="home-today">{summary}</p>}
        {auditError && timeline.lines.length > 0 && (
          // Live lines are on screen, but the history behind them is unread: the page says which
          // half it is showing instead of letting the list stand for the whole day.
          <p className="hint" role="status">
            The console could not read this machine's audit, so this timeline shows only calls that
            arrive live.
          </p>
        )}
        <ActivityTimeline
          lines={timeline.lines}
          devices={props.devices}
          capped={ownWindowCut}
          // The hub's timeline is already this machine's day: a machine chip here could only say
          // "All machines" over one machine's lines, which would be a lie.
          showFilters={false}
          onWatch={props.onWatchTerminal}
          empty={empty}
        />
        <WindowRead readAt={readAt} fetchedNewest={fetchedNewest} liveNewest={liveNewest} />
      </section>

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
    </div>
  );
}

export type HomePageProps = FleetProps & {
  /// The machine in context. Null is the fleet zero state; a chosen machine is the hub.
  device: Device | null;
  onConnect: () => void;
  onTools: () => void;
  onActivity: () => void;
  /** The console's own build, from /health; the machine chip next door is the device's. */
  consoleVersion?: string | null;
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
