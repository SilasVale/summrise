import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { LiveNow } from "./LiveNow";
import type { LiveLine, StartedCall } from "./liveActivity";
import { api, type AuditLine, type Device } from "./api";
import { AttemptReason, attemptCount, attemptReason } from "./attempt";
import { clockTime, coverageClause, lastHeardMs, seenAgo, shortId } from "./format";
import { listPlugins } from "./pluginApi";
import { surfaceHash } from "./surface-link";
import { toolsReadState } from "./toolsRead";
import { useLiveTools, type LiveToolsRetry } from "./useLiveTools";
import { ACTIVITY_LIMIT, mergeActivity } from "./activityMerge";
import { isClientCall } from "./activitySummary";
import { auditFailed } from "./auditFilter";
import { spanText } from "./age";
import { RecoveryLine } from "./RecoveryLine";
import { StartedLine } from "./StartedLine";

/**
 * The selected machine's own page: the `Overview` tab of Device.
 *
 * Home is the fleet's verdict; this is the machine's live view — its status, its Now band, its
 * counts line and one line about today. It lives under Device because the machine's own page is
 * Device; Home never becomes this page. The day line states the day, its count and its span;
 * the calls themselves are Activity's, once, because a second copy of the record here made the
 * machine's front door a scoped Activity (audit-42 F1).
 */
export type OverviewPageProps = {
  device: Device;
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
  /**
   * When this console process began, from `/health`'s uptime. The recovery line uses it to tell
   * the machine's absence from the console's own downtime inside the gap.
   */
  consoleStartedAt?: number | null;
};

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

/** Midnight in the console's own timezone; the hub's day is today's, not the audit window's. */
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
 * The day's one line: the day, its count and the span its calls cover.
 *
 * **THE COUNT IS THE DAY'S, THE SPAN IS THE WINDOW'S.** The count and the failures come from
 * `/fleet`'s `today`, the whole local calendar day (the same midnight the record cuts at); the
 * fetched window only says how long that work lasted, because the call rows live in Activity now.
 * A span is stated only when the window can prove it holds every one of today's calls: a cut
 * window, or one that has not answered yet, may be missing the day's first call, so the line
 * states the count alone rather than a shorter day. When the day read cannot prove the whole day,
 * `coverageClause` says `since` or the scan bound — a partial day is stated, not hidden.
 *
 * A day with one call has no span, so it names the call's moment instead. A window with no line
 * today still knows when the machine last worked, so it says that instead of letting the empty
 * day read as a machine that did nothing overnight.
 */
function todayLine(
  day: Device["today"],
  lines: readonly AuditLine[],
  partial: boolean,
  now = Date.now()
): string {
  const calls = lines.filter(isClientCall);
  const start = startOfToday(now);
  const today = calls.filter((line) => line.ts_ms >= start);
  const newestToday = today.reduce<AuditLine | null>(
    (latest, line) => (latest === null || line.ts_ms > latest.ts_ms ? line : latest),
    null
  );
  const oldestToday = today.reduce<AuditLine | null>(
    (oldest, line) => (oldest === null || line.ts_ms < oldest.ts_ms ? line : oldest),
    null
  );
  // The calls' own first and last moment. It is the day's only when the window is whole: a cut or
  // unread window's oldest today line is not the day's first call.
  const span =
    !partial && oldestToday !== null && newestToday !== null
      ? Math.max(0, newestToday.ts_ms - oldestToday.ts_ms)
      : 0;
  const spanClause = span > 0 ? ` · spanning ${spanText(span)}` : "";
  const oneCallClause =
    span === 0 && today.length === 1 && newestToday !== null
      ? ` · last ${clockTime(newestToday.ts_ms)}`
      : "";
  if (day && day.client_calls > 0) {
    return (
      `Today ${plural(day.client_calls, "client call")}` +
      (day.failed > 0 ? ` · ${day.failed} failed` : "") +
      spanClause +
      oneCallClause +
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
  const failed = today.filter(auditFailed).length;
  return (
    `Today ${plural(today.length, "client call")}` +
    (failed > 0 ? ` · ${failed} failed` : "") +
    spanClause +
    oneCallClause
  );
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
 * The operator's press, in words, once its read settled.
 *
 * The five-second poll keeps reading the same routes; it is deliberately silent, because a sentence
 * that appeared on every tick would be noise. A person who pressed Retry is owed the opposite: the
 * moment their attempt finished and what it found, even when the answer is the same one already on
 * screen. Without it the press and the poll were the same 4,208-character page (audit-38 F3).
 */
function retryOutcome(retried: LiveToolsRetry | null, offline: boolean): string | null {
  if (retried === null) return null;
  if (retried.ok) return `The console reached the machine · checked ${clockTime(retried.at)}.`;
  return offline
    ? `Still offline · last checked ${clockTime(retried.at)}.`
    : `Still failing · last checked ${clockTime(retried.at)}.`;
}

/**
 * The machine's live page: is it running, what is it doing now, what did it do today. One status
 * line, the Now band, one counts line and one day line with the record's door. The band states
 * presence and draws none of the calls; the day line states the day, its count and its span, and
 * the rows themselves are Activity's, once (audit-41 F1, audit-42 F1). Identity is the Device
 * header's (the topbar names the machine); this page does not repeat the destination's h1. The
 * console's endpoint lives in Connect, so this page does not name it; the offline advice names the
 * machine's own panel as a place on the machine, not a door from here.
 */
export function OverviewPage(props: OverviewPageProps) {
  const device = props.device;
  const name = device.name ?? shortId(device.id);
  const live = useLiveTools(device.id);
  // One reading/failed/ready state for the counts line. A zero is drawn only when the read
  // answered with no tools; while it is on the wire the line says so.
  const read = toolsReadState(live);
  // A stale catalogue still names the tool surface, so the plugin read is allowed; an empty list
  // after a failed read knows nothing and must not be read as "an older build".
  const known = live.tools.length > 0 || (!live.loading && live.error === null);
  const pluginsTool = live.tools.some((tool) => tool.name === "plugins.list");
  const plugins = usePluginCount(device.id, known, pluginsTool);
  // The page's subject is the machine: the stream's lines are the newest of the same audit, and
  // both the Now band and the day line drop every other machine's. The day line is today's; older
  // days belong to Activity, behind the All activity link.
  const liveLines = (props.liveLines ?? []).filter((line) => line.device_id === device.id);
  // The band is this machine's, like the day line: another machine's in-flight call must not name
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
        setMachineAudit({ device: device.id, lines });
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
  // The fetched window and the live stream are one list: the day's span must count the calls that
  // arrived live as well as those already in the file, and the Now band reads presence from the same
  // merged window.
  const windowLines = mergeActivity(fetched, liveLines).sort((a, b) => b.ts_ms - a.ts_ms);
  // The window keeps older lines too: the day line needs one of them to say when the machine last
  // worked, and the empty sentence needs today's rows to know what it is missing.
  const dayLines = windowLines.filter((line) => line.ts_ms >= todayStart);
  const clientRows = dayLines.filter(isClientCall);
  // **A CUT WINDOW HAS NO PROVABLE SPAN.** The fetched answer is this machine's newest 1000 lines.
  // When every one of them is today, the day's first call may be older than the window's oldest
  // line, so a span counted from here would start late. Otherwise the oldest fetched line is
  // yesterday (or the answer was short), and every today line is in hand.
  const oldestFetched = fetched[fetched.length - 1];
  const windowCut =
    fetched.length >= ACTIVITY_LIMIT &&
    oldestFetched !== undefined &&
    oldestFetched.ts_ms >= todayStart;
  // Before the first window arrives, `/fleet`'s day read is still a fact; once a window (or a live
  // line) is in hand, the line also states the span. A failed read leaves the count it had and
  // drops the span rather than letting a stale window stand for the day.
  const summary =
    device.today != null || loaded || liveLines.length > 0
      ? todayLine(device.today, windowLines, !loaded || windowCut, now)
      : null;
  // The empty sentence names what is missing rather than claiming nothing happened. A window whose
  // only rows are the console's or unattributed is not the AI's work either; the day's own read may
  // still prove calls the window does not hold, so it is not called empty in that case.
  const noCallsKnown =
    loaded && clientRows.length === 0 && (device.today?.client_calls ?? 0) === 0;
  const empty = auditError
    ? "The console could not read this machine's activity, so this day line may be incomplete."
    : !loaded && liveLines.length === 0
      ? "Reading this machine's activity…"
      : noCallsKnown
        ? dayLines.length === 0
          ? `No calls from ${name} today. When an AI client calls this machine with its token, ` +
            "its work appears in Activity; Connect a client sets one up."
          : `No client calls from ${name} today.`
        : null;

  const offline = !device.online || live.offline;
  // **THE OFFLINE PAGE IS DATED, LIKE EVERY OTHER MACHINE SURFACE.** The Machine tab, Home's rows
  // and the shell's notice all say how long the machine has been away; this page said only that it
  // was away and then drew a Now band claiming a quiet present (audit-43 F5). The notice states the
  // newest evidence the console has, the same moment `lastHeardMs` gives every other surface.
  const heard = lastHeardMs(device);
  const lastSeen = heard === null ? "" : ` Last seen ${seenAgo(heard)}.`;
  // The last operator press, not the poll: null until Retry has settled once on this machine.
  const retryNote = retryOutcome(live.retried ?? null, offline);
  // **ONE PLACE FOR EACH BUILD.** The subtitle used to be "Running · console v0.3.273-ux43", while
  // the Account menu already prints "Console 0.3.273-ux43"; at 390 the header chip drops the
  // device's own "v0.3.274", which made the console's the page's only build — the menu's fact said
  // twice. The subtitle says what the machine is; the chip and the menu own the versions. Its word
  // is the fleet's `online`/`offline`, the same one Home's rows and the topbar now state, not the
  // app's `Running` (audit-43 F2).
  const statusLine = offline ? "offline" : "online";
  const capabilities = new Set(live.tools.map((tool) => tool.capability)).size;
  // THE FAILURE SENTENCE SAYS WHY. "Not connected" alone leaves the operator choosing between the
  // edge and the machine; the attempt count and the device's own reason are that choice. The reason
  // is cut to one line and titled, because a phone-width banner has room for the count but not for a
  // 51-character channel sentence. A device that gave no reason keeps the sentence it had.
  const failedReason = attemptReason(device.last_attempt);
  const failedAttempts = device.last_attempt?.attempts ?? 0;
  const notice: ReactNode = offline
    ? (
        <>
          This machine is offline.{lastSeen}{" "}
          {failedReason !== ""
            ? (
                <>
                  {attemptCount(failedAttempts)} so far, the last failed —{" "}
                  <AttemptReason reason={failedReason} />.{" "}
                </>
              )
            : null}
          <OfflineAdvice />
        </>
      )
    : live.error
      ? live.tools.length === 0
        ? "The machine is running. The console could not read its tool list, so the counts are not shown."
        : "The machine is running. The console could not read its tool list, so these counts may be stale."
      : null;

  return (
    <div className="overview-page">
      {/* **A PAGE HAS ONE HEAD, AND THIS IS A CARD (the sixty-second list).** Measured: the Device page
          drew `.page-head.device-head` at y=76 and this block — a `<header>` — at y=133, so two headers
          sat 57 px apart and the second one's `h2` read as a second page title. The machine's name and
          its start are the SUBJECT of a card: the same `card` grammar Home's `.machine-summary` uses,
          whose heading is the machine's name. A `<section aria-label>` with an `h2` inside it is the
          same shape as that card, and a page keeps the one head it is allowed. */}
      <section className="card overview-head" aria-label={`${name} — this machine`}>
        <div>
          <h2>{name}</h2>
          <p className="page-sub">{statusLine}</p>
          {/* **THE COUNTS ARE THE MACHINE'S OWN LINE, NOT A FOURTH BLOCK (the hundred-and-forty-sixth list).** Measured at
              1440 on the live page 2026-11-12: this page drew FOUR bordered blocks stacked in one column — the machine card,
              the Now band, this counts chip and the day row — four borders around four single lines, which is what makes a
              page read as four things instead of one. The counts answer "what can this machine do", so they belong in the
              card that names it; the chip's own grammar (a button to the Tools tab) does not change. */}
          {offline ? (
            <p className="hint">Tool counts are unavailable while the machine is offline.</p>
          ) : read === "reading" ? (
            <p className="hint" role="status">
              Reading the tool list…
            </p>
          ) : read === "failed" ? null : (
            <button type="button" className="counts-line" onClick={props.onTools}>
              {plural(live.tools.length, "tool")} · {plural(capabilities, "capability", "capabilities")}
              {plugins !== null ? ` · ${plural(plugins, "plugin")}` : ""}
              {live.stale ? " · stale" : ""} <span aria-hidden="true">→</span>
            </button>
          )}
        </div>
        {/* The device's own account of its last start, in the machine's head beside the name it
            describes (audit-44 F1). It used to be the shell's first line, above the h1 and 35 px
            above this name. The recovery line is its other half: how long the machine was away
            before this start, when the console's pair or the device's journal proves it (F12). */}
        <div className="overview-start">
          <StartedLine device={device} />
          <RecoveryLine device={device} consoleStartedAt={props.consoleStartedAt} />
        </div>
      </section>

      {notice && (
        <div className="notice" role="status" title={live.error ?? undefined}>
          <span>
            {notice}
            {retryNote !== null && (
              <>
                {" "}
                <span
                  className="hint retry-outcome"
                  title={live.retried?.error ?? undefined}
                >
                  {retryNote}
                </span>
              </>
            )}
          </span>
          <button type="button" disabled={live.retrying} onClick={() => live.retry()}>
            {live.retrying ? "Retrying…" : "Retry"}
          </button>
        </div>
      )}

      {/* **A BAND THAT ASKS "NOW" IS NOT DRAWN FOR A MACHINE THAT IS AWAY.** It said `Quiet` and
          `no calls in the last minute` for a machine the console could not hear: absence of
          evidence read as evidence of idleness (audit-43 F5). The dated notice above is the
          machine's state; the day line below is its last record. */}
      {!offline && (
        <LiveNow
          // The band and the day line are the same machine's day: feeding the band the fleet's
          // newest 200 live lines made it say "Quiet" while the window held this machine's calls,
          // and after a reconnect the live list is only the newest forty. The band reads presence
          // from that window and draws none of its rows: the calls themselves live in Activity.
          lines={dayLines}
          running={liveRunning}
          state={props.liveState ?? "connecting"}
        />
      )}

      {/* **THE DAY IS ONE LINE, NOT ACTIVITY'S CARD.** The measured page ended in a 29-row copy of
          Activity's Today card: the same timestamps, the same `2 calls · 17 s` clusters, the same
          heading (audit-42 F1). The day's count, failures and span stay here because they are the
          machine's state; the rows are the record's and live in Activity, behind this one door. It
          is not a live region: it moves on the five-second reread, not on every line. */}
      <section className="panel home-day" aria-label="This machine's activity">
        {summary !== null && <p className="home-today">{summary}</p>}
        <button type="button" className="link home-all" onClick={props.onActivity}>
          All activity <span aria-hidden="true">→</span>
        </button>
        {empty !== null && <p className="hint">{empty}</p>}
      </section>
    </div>
  );
}
