import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import logoUrl from "./logo.svg";
import {
  api,
  HttpError,
  NoAnswer,
  type AuditDay,
  type AuditLine,
  type AuditSource,
  type Device,
  type Session,
  type TokenMeta
} from "./api";
import { copyText } from "./copy";
import { lastHeardMs, shortId } from "./format";
import { retrySentence } from "./logic";
import { recordVitals, type VitalsSample } from "./vitals";
import { AUDIT_WINDOW, AuditTable, type AuditScope } from "./AuditTable";
import { type AuditRange, type RangeFacts } from "./auditTime";
import { auditFailed, type AuditFilter } from "./auditFilter";
import { mergeActivityWindow, type WindowCounts } from "./activityMerge";
import { activityDaySentence } from "./activitySummary";
import { ActivityTimeline } from "./ActivityTimeline";
import { useJoinMarks } from "./useJoinMarks";
import { machineCounts, mergeMachineRows } from "./machineRows";
import { useMachineRows } from "./useMachineRows";
import { DEVICE_TAB_LABELS, PAGE_LABELS, Rail, type DeviceTab, type Page } from "./Rail";
import { Modal } from "./Modal";
import { ConnectSheet, type ConnectSection } from "./ConnectSheet";
import { TerminalWorkspace } from "./TerminalWorkspace";
import type { Grant } from "./terminal";
import { FilesPage } from "./FilesPage";
import { RunsPage } from "./RunsPage";
import { parseRunLink, runLink } from "./run-link";
import { parseSurfaceHash, surfaceHash } from "./surface-link";
import { pageVersion } from "./consoleVersion";
import { ScreenPage } from "./ScreenPage";
import { MemoryPage } from "./MemoryPage";
import { SystemPage } from "./SystemPage";
import { HomePage, type FleetRead } from "./HomePage";
import { OverviewPage } from "./OverviewPage";
import { useLiveActivity } from "./useLiveActivity";
import { ToolsTab } from "./ToolsTab";
import { DevicePageShell, deviceJournal } from "./DevicePageShell";
import { AccountMenu } from "./AccountMenu";
import { MachineMenu } from "./MachineMenu";
import { useMediaQuery } from "./useMediaQuery";

/// The selected machine survives a reload; it is deliberately not in the URL. A deep link to a
/// device-specific surface without one lands on the picker instead.
const SELECTED_KEY = "summrise.selected.device";

/// The sign-in panel's address. The panel used to be reachable only through Account → Sign out, so a
/// reader told "the sign-in page" had no URL to try; `#signin` draws it (and the sign-out gesture
/// leaves this address behind, audit-39 F6).
const SIGN_IN_HASH = "#signin";

function readSelectedDevice(): string | null {
  try {
    return localStorage.getItem(SELECTED_KEY);
  } catch {
    return null;
  }
}

function storeSelectedDevice(id: string | null) {
  try {
    if (id === null) localStorage.removeItem(SELECTED_KEY);
    else localStorage.setItem(SELECTED_KEY, id);
  } catch {
    /* a browser that refuses storage still gets a working session, minus the remembered machine */
  }
}

/**
 * One named failure on the shell's banner.
 *
 * `key` is the action, not the sentence: a repeated press during one refusal is the same fact.
 * Replacing the entry by key is the dedupe (audit-38 F2 measured the old banner reading `The console
 * could not read Failed to fetch; Failed to fetch`).
 *
 * `detail` is evidence, never the sentence: the browser's own `Failed to fetch`, or the console's
 * status and code. It is the entry's title, where a curious operator can read it and a sentence
 * cannot present it as the console's words.
 *
 * **THE PAGE OWNS A FAILED READ.** Home, Activity and the Device picker each hold the one statement
 * and the one Retry for the read they draw; a banner for the same read was a second statement of
 * one failure (audit-44 F3). What is left here is an action's refusal — a rename, a remove, an
 * enrollment code, a refused sign-in — which has no page surface below it. The banner has no Retry:
 * the action is the page's own control, and a second button for the same read was audit-40 F4.
 */
type BannerFailure = {
  key: string;
  sentence: string;
  detail?: string;
};

/**
 * What Activity knows about the audit window it is meant to draw.
 *
 * `reading` is before the first answer for this scope, `failed` is a read that did not arrive with
 * the console's reason, and `ready` is a window in hand. The page's sentence and its notice are
 * derived from this, not from `audit.length`: an empty array is both "nothing happened" and
 * "nothing read", and audit-38 F6 measured the page claiming to follow the record it had not read.
 */
type AuditRead =
  | { state: "reading" }
  | { state: "failed"; reason: string }
  | { state: "ready" };

export function App() {
  const [session, setSession] = useState<Session | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  /**
   * The latest `/fleet` read: still reading, failed with its reason, or the list it brought.
   *
   * `devices` alone cannot say which: an empty array is both "no machines" and "nothing read yet",
   * and the fleet page must not turn the second into the first (audit-38 F1). A failed read keeps
   * the last list on screen but marks it stale; only a successful answer sets `ready`.
   */
  const [fleetRead, setFleetRead] = useState<FleetRead>({ state: "reading" });
  // The hub rereads its machine's day on this page's clock. Each poll is one tick; the hub turns a
  // new tick into one read of its own window, so the day below the band is never a mount-time memory.
  const [pollTick, setPollTick] = useState(0);
  // The browser-local history sampled from the same fleet the page polls, one entry per device id.
  // It lives in localStorage; this state is the copy the pages draw.
  const [vitals, setVitals] = useState<Record<string, VitalsSample[]>>({});
  const [audit, setAudit] = useState<AuditLine[]>([]);
  /**
   * The machine whose window is in `audit`; `null` is the fleet's. The page reads a machine's own
   * window when its scope names one: filtering the fleet's newest window in the page lets another
   * machine's newer lines push this machine's older ones out of it.
   */
  const [auditDevice, setAuditDevice] = useState<string | null>(null);
  /** The device the newest read asked for; it differs from `auditDevice` until that read answers. */
  const [auditReadDevice, setAuditReadDevice] = useState<string | null>(null);
  /**
   * The local day's counts, from `/audit/day`, and the scope they answer for.
   *
   * The header counts the day, not the window: the fetched rows are capped at 1,000 and slide, so a
   * count derived from them shrank during the day (the measured 269 against the file's 649). This is
   * the same read the fleet serves as each machine's `today`.
   */
  const [daySummary, setDaySummary] = useState<{
    device: string | null;
    day: AuditDay;
  } | null>(null);
  /**
   * The file the window is read from, from `/audit/source`. It is the same file whatever the scope,
   * so a scope change does not refetch it for a different answer; a failed read leaves `null` and
   * the cap sentence's neighbour draws nothing rather than a guess.
   */
  const [auditSource, setAuditSource] = useState<AuditSource | null>(null);
  /**
   * The audit read's own state, so Activity can say it is reading, that it failed with a reason, or
   * that it is following the record. It is a separate fact from `audit` and `devices`: a slow audit
   * must not hold the fleet back, and a failed one must not leave the page claiming the record.
   */
  const [auditRead, setAuditRead] = useState<AuditRead>({ state: "reading" });
  /**
   * The scope whose window the latest successful read put in hand; `undefined` before any answer.
   * A failed read never moves it, which is how the page tells "no window at all" from "the last
   * window, plus a failed reread".
   */
  const auditAnsweredDevice = useRef<string | null | undefined>(undefined);
  /**
   * Read the audit window again into the page.
   *
   * One function for every reader: the five-second poll, the stream's catch-up after a dropped
   * frame, and the scope change. They ask for the same `AUDIT_WINDOW` and the same device scope, so
   * a line the page reads back is the line the poll would have read; neither can quietly narrow the
   * window while the other widens it.
   *
   * Both reads can overlap. The file only gains lines and both ask for its newest window, so a
   * response from a read that started later is the fresher answer; a response from an older read
   * that lands after it is dropped rather than allowed to reopen a hole the newer read just filled.
   * A failed read never counts as an answer, so it cannot hide an older read that did answer.
   */
  const auditReadStarted = useRef(0);
  const auditReadApplied = useRef(0);
  // The scope's device as of the latest render; the readers use the ref so none of them can fetch a
  // window the page is not showing.
  const auditWindowDevice = useRef<string | null>(null);
  const rereadAudit = useCallback(async () => {
    const device = auditWindowDevice.current;
    const generation = ++auditReadStarted.current;
    setAuditReadDevice(device);
    // A scope with no answer yet is being read, and the page says so. A poll over a window already
    // in hand keeps saying it is following: a healthy five-second read must not blank the sentence.
    if (auditAnsweredDevice.current !== device) setAuditRead({ state: "reading" });
    // One snapshot, three reads: the rows the page draws from the window, the day those rows
    // belong to, and the file both were read from. Asking together keeps the header, the list and
    // the source line on the same clock; the window may be smaller than the day, and that is
    // exactly what the two numbers are allowed to say.
    try {
      const [lines, day, source] = await Promise.all([
        device === null ? api.audit(AUDIT_WINDOW) : api.audit(AUDIT_WINDOW, device),
        // **THE DAY IS A COMPANION FACT TOO (the hundred-and-thirty-first list).** It was read inside
        // the same `Promise.all` without a catch, so an answer the client refuses — a 200 that carries
        // no counts — took the WINDOW with it: a thousand readable rows vanished and the page said
        // `this timeline shows only calls that arrive live`. Rows without a day's count are what the
        // page already draws while a scope is being replaced; no rows because a copy failed is not.
        api.auditDay(device ?? undefined).catch(() => null),
        // The source line is a companion fact, not the window: a failed read here must not fail the
        // rows, and the page then says nothing about the file rather than guessing.
        api.auditSource().catch(() => null)
      ]);
      if (generation > auditReadApplied.current) {
        auditReadApplied.current = generation;
        auditAnsweredDevice.current = device;
        setAudit(lines);
        setAuditDevice(device);
        setDaySummary(day === null ? null : { device, day });
        setAuditSource(source);
        setAuditRead({ state: "ready" });
      }
    } catch (cause) {
      // A failed read never counts as an answer, so it cannot hide an older read that did answer.
      // The reason is the page's, and the window already in hand stays where it is.
      if (generation > auditReadApplied.current) {
        setAuditRead({ state: "failed", reason: describe(cause) });
      }
      throw cause;
    }
  }, []);
  // The same index, streamed: what the AI is doing now, not what it did before the last read.
  // `rereadAudit` is how the hook fills the hole a `lagged` frame says the stream left.
  const live = useLiveActivity(Boolean(session?.authenticated), rereadAudit);
  // A linked range is read before the sign-in: the fragment is the page's address, and only the
  // Table can show a range, so such a link opens the Table rather than the live timeline.
  const linkedRange = parseSurfaceHash(window.location.hash)?.range ?? null;
  // Activity follows the stream unless the operator paused it: a paused table is a snapshot of the
  // list at that moment, and the lines that arrived since are counted rather than lost.
  const [activityPaused, setActivityPaused] = useState(false);
  const [activityView, setActivityView] = useState<"stream" | "table">(
    linkedRange ? "table" : "stream"
  );
  // The "Clients only" control belongs to the page, not only to the timeline: the choice survives a
  // switch to the table and back. The header stays the day's sentence; the control counts its own
  // fetched window and says so, so the page never makes a second `client calls` claim.
  const [activityClientsOnly, setActivityClientsOnly] = useState(false);
  // The filters live above both views: the timeline's search is the table's tool field and its
  // machine chip is the table's device filter, so switching views cannot drop either of them.
  const [activityFilter, setActivityFilter] = useState<AuditFilter>({
    device: "",
    token: "",
    tool: "",
    status: "all"
  });
  const [activityRange, setActivityRange] = useState<AuditRange | null>(linkedRange);
  // The range's answer, and whether its read failed. A range is its own request: the page's
  // five-second poll still reads the newest window for the timeline and the day header.
  const [rangeAnswer, setRangeAnswer] = useState<{
    range: AuditRange;
    device: string | null;
    lines: AuditLine[];
    facts: RangeFacts;
  } | null>(null);
  const [rangeError, setRangeError] = useState(false);
  const rangeReadStarted = useRef(0);
  const rangeReadApplied = useRef(0);
  // `null` follows the header's machine, which is the scope the table opens on; after the operator
  // picks a scope or a chip, the choice stays explicit for this page's lifetime.
  const [activityScope, setActivityScope] = useState<AuditScope | null>(null);
  const [frozenActivity, setFrozenActivity] = useState<AuditLine[] | null>(null);
  // Whether the frozen list itself was cut at the cap. The sentence above the rows must describe
  // what is on screen, not what the merge would be if the page were following again.
  const [frozenCapped, setFrozenCapped] = useState(false);
  // Whether the window frozen behind the pause filled the server's limit. A paused page keeps
  // saying what its window is, not what the live window has since become.
  const [frozenAtLimit, setFrozenAtLimit] = useState(false);
  // The window's counts at the freeze, so a paused page keeps the numbers the sentence had, not the
  // numbers the sliding window would give the same list. `null` while following.
  const [frozenCounts, setFrozenCounts] = useState<WindowCounts | null>(null);
  // The day at the pause, so a paused header keeps the numbers the frozen list had. `null` while
  // following; a pause with no day answer yet keeps the header absent rather than inventing one.
  const [frozenDay, setFrozenDay] = useState<{
    device: string | null;
    day: AuditDay;
  } | null>(null);
  // The frozen list's window. The cap sentence and the loading decision describe what is on screen,
  // and a pause keeps the rows it froze even if a scope change fetches another window behind it.
  const [frozenDevice, setFrozenDevice] = useState<string | null>(null);
  // The moment the freeze was taken, so the count is "lines newer than this" rather than a length
  // difference: the fetched window slides, and a difference under-reports once it does.
  const [pausedAt, setPausedAt] = useState<number | null>(null);
  const activityWindow = mergeActivityWindow(audit, live.lines);
  const activityLines = activityWindow.lines;
  const shownActivity = activityPaused && frozenActivity ? frozenActivity : activityLines;
  const shownCapped =
    activityPaused && frozenActivity ? frozenCapped : activityWindow.capped;
  // The window's limit is the fetch's fact, so a paused page keeps the one it froze with.
  const shownAtLimit =
    activityPaused && frozenActivity ? frozenAtLimit : activityWindow.atLimit;
  // **THE SENTENCE COUNTS WHAT THE LIST DID.** The timeline drops channel boundaries; the table is
  // the audit and draws them. Both withheld repeated keys. The counts come from the same merge the
  // rows do, so the fetched figure cannot disagree with the drawn one.
  const activityCounts: WindowCounts = {
    fetched: activityWindow.fetched,
    drawn: activityWindow.lines.length - activityWindow.boundaries,
    boundaries: activityWindow.boundaries,
    repeats: activityWindow.repeats
  };
  const tableCounts: WindowCounts = {
    fetched: activityWindow.fetched,
    drawn: activityWindow.lines.length,
    boundaries: 0,
    repeats: activityWindow.repeats
  };
  const shownCounts = activityPaused && frozenCounts ? frozenCounts : activityCounts;
  const shownTableCounts =
    activityPaused && frozenCounts && frozenActivity
      ? { ...frozenCounts, drawn: frozenActivity.length, boundaries: 0 }
      : tableCounts;
  const freshActivity =
    activityPaused && pausedAt !== null
      ? activityLines.filter((line) => line.ts_ms > pausedAt).length
      : 0;
  /**
   * **WHERE THE FAILURES CONCENTRATE (the hundred-and-eighty-sixth list).** The header says how many failed; nothing said WHERE. A
   * person reading `51 failed` needs the three tools doing most of it before they can act, and the window already holds them — this
   * groups the failed client exchanges by tool name, newest window, and draws the top three. Each is a button that sets the tool
   * filter AND the failures filter, so the number is one click from its rows (the same two filter fields the controls own).
   */
  const failureTools = (() => {
    const counts = new Map<string, number>();
    for (const line of shownActivity) {
      if (!auditFailed(line)) continue;
      const name = line.tool ?? "";
      if (name === "") continue;
      counts.set(name, (counts.get(name) ?? 0) + 1);
    }
    return [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 3);
  })();
  const pauseActivity = () => {
    if (activityPaused) {
      setActivityPaused(false);
      setFrozenActivity(null);
      setFrozenCapped(false);
      setFrozenAtLimit(false);
      setFrozenCounts(null);
      setFrozenDay(null);
      setFrozenDevice(null);
      setPausedAt(null);
    } else {
      setPausedAt(Date.now());
      setFrozenActivity(activityLines);
      setFrozenCapped(activityWindow.capped);
      setFrozenAtLimit(activityWindow.atLimit);
      setFrozenCounts(activityCounts);
      setFrozenDay(daySummary);
      setFrozenDevice(auditDevice);
      setActivityPaused(true);
    }
  };
  // The operator's permanent tokens, fetched so the audit can name the token behind a call and
  // Connect can list what exists. Empty when the console would not answer: the table falls back to
  // record ids.
  const [tokens, setTokens] = useState<TokenMeta[]>([]);
  // A run link in the fragment survives a reload (and the sign-in a reload needs). It names the
  // machine as well as the run, because a run id is only unique on its device.
  const [initialRun] = useState(() => parseRunLink(window.location.hash));
  // The machine is read once, before the fragment: the two retired rail destinations only become the
  // machine's Tools tab when there is a machine to put them under.
  const [initialSelected] = useState<string | null>(
    () => initialRun?.deviceId ?? readSelectedDevice()
  );
  const [initialSurface] = useState(() =>
    parseSurfaceHash(window.location.hash, { machineSelected: initialSelected !== null })
  );
  const [page, setPage] = useState<Page>(() =>
    initialRun ? "device" : (initialSurface?.page ?? "home")
  );
  const [tab, setTab] = useState<DeviceTab>(() =>
    initialRun ? "runs" : (initialSurface?.page === "device" ? initialSurface.tab ?? "terminal" : "terminal")
  );
  // Screen's second view is addressable: `#device/screen/browser` (and `#device/browser`) opens it,
  // and the Screen header's switch writes it back. It is not a sixth Device tab.
  const [screenView, setScreenView] = useState<"desktop" | "browser">(() =>
    initialSurface?.page === "device" && initialSurface.tab === "screen"
      ? initialSurface.view ?? "desktop"
      : "desktop"
  );
  const [openTool, setOpenTool] = useState<string | null>(
    initialSurface?.page === "device" && initialSurface.tab === "tools"
      ? initialSurface.tool ?? null
      : null
  );
  const [selected, setSelected] = useState<string | null>(initialSelected);
  const mainRef = useRef<HTMLElement>(null);
  const mounted = useRef(false);
  // A run id handed from Activity to Runs; RunsPage consumes it once and clears it.
  const [runsFocus, setRunsFocus] = useState<string | null>(initialRun?.runId ?? null);
  /**
   * Whether the address names the sign-in panel. It is a view of its own, not a fourth destination:
   * the rail still has three, and a successful sign-in returns to the surface the page was on.
   */
  const [signInRoute, setSignInRoute] = useState(
    () => window.location.hash === SIGN_IN_HASH
  );
  /**
   * The shell's named failures, one entry per read and replaced by that read's next failure.
   *
   * THE OLD STATE WAS ONE STRING OF RAW CAUSES: `The console could not read Failed to fetch`, and
   * two failed reads joined with `; ` (audit-38 F2). A failure now carries its subject, the
   * browser's text only as the entry's title; the page's notice owns the one Retry.
   */
  const [failures, setFailures] = useState<BannerFailure[]>([]);
  const [busy, setBusy] = useState(false);
  const [enrolling, setEnrolling] = useState(false);
  const [tokenInput, setTokenInput] = useState("");
  /** Whether the console let this tab in through the proxy's identity rather than a token. */
  const [proxied, setProxied] = useState(false);
  const [renaming, setRenaming] = useState<Device | null>(null);
  const [removing, setRemoving] = useState<Device | null>(null);
  const [connect, setConnect] = useState<{
    section: ConnectSection;
    device: Device | null;
    /** Scroll the Tokens section to this record, when a row's Manage asked for it. */
    tokenId?: string | null;
  } | null>(null);
  const [draftName, setDraftName] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const [code, setCode] = useState<{ code: string; expires_at_ms: number } | null>(null);
  const [grant, setGrant] = useState<Grant | null>(null);
  /** True when the console refused a call on the session in force, so the recovery panel is offered above the live tabs. */
  const [grantStale, setGrantStale] = useState(false);
  const [consoleVersion, setConsoleVersion] = useState<string | null>(null);
  /**
   * **A TAB FROM BEFORE THE DEPLOY IS TOLD SO (the hundred-and-ninety-ninth list).** Measured: three console-originated refusals in one
   * audit window (`monitor.add`, `memory.list`, `monitor.status`), all from a page whose build had already been fixed. The page's own
   * stamped version against `/health`'s answer is enough — no second read, no new poll.
   */
  const ownBuild = pageVersion();
  const newerBuild = ownBuild !== null && consoleVersion !== null && consoleVersion !== ownBuild ? consoleVersion : null;
  // **THE COMPARISON HAS TO KEEP HAPPENING.** `/health` is read on a session probe, which a tab that stays open does not repeat: the
  // banner would only ever appear on a page loaded after the deploy, which is the page that does not need it. One cheap read a
  // minute keeps the label right and lets an old tab notice it is old.
  useEffect(() => {
    if (ownBuild === null) return;
    let current = true;
    const check = async () => {
      try {
        const health = await api.health();
        if (current) setConsoleVersion(health.version);
      } catch {
        /* the label keeps its last answer; a failed read is not news about the build */
      }
    };
    const timer = window.setInterval(() => void check(), 60_000);
    return () => {
      current = false;
      window.clearInterval(timer);
    };
  }, [ownBuild]);
  /** When this console process began, from `/health`'s uptime; null until the answer lands. */
  const [consoleStartedAt, setConsoleStartedAt] = useState<number | null>(null);
  const [accessNote, setAccessNote] = useState<string | null>(null);
  const phone = useMediaQuery("(max-width: 639px)");
  const chosen = devices.find((device) => device.id === selected) ?? null;
  /**
   * A selected id the fleet answered without is not a machine: it is a stale bookmark, or one the
   * fleet row's Remove ended. Home and Device already fall back (the fleet page, the picker) by
   * drawing `chosen === null`; Activity kept the id as its scope and drew it as `unknown machine ·
   * <hex>` (audit-38 F8). The fallback is the same everywhere; only the sentence is Activity's,
   * because that is the page whose filter used to stand for a machine.
   *
   * It waits for a successful `/fleet`: while that read is pending or failed the list in hand is
   * not proof the id is gone, and the wrong fallback would be worse than the stale filter.
   */
  const selectedGone =
    selected !== null &&
    fleetRead.state === "ready" &&
    !devices.some((device) => device.id === selected);
  // The selection Activity may act on. A gone id is no selection, so its window is the fleet's.
  const activeDevice = selectedGone ? null : selected;
  // The scope defaults to the chosen machine and follows it while it stays implicit; the timeline
  // shows the same device the table is applying, so neither view can silently disagree. No machine
  // is "All machines", even if an earlier scope choice named one: a fallback cannot keep a control
  // pressed for a machine that is not there.
  const activityScopeNow: AuditScope =
    activeDevice === null ? "all" : (activityScope ?? "machine");
  const activityMachine =
    activityScopeNow === "machine" && activeDevice ? activeDevice : activityFilter.device;
  // THE SCOPE IS THE WINDOW. The rows are narrowed to this machine, and when there is one the read
  // asks the console for that machine's newest lines rather than filtering the fleet's window: with
  // `limit` 3, a machine's six older lines can sit entirely behind another machine's eight newer ones.
  const auditScopeDevice = activityMachine === "" ? null : activityMachine;
  auditWindowDevice.current = auditScopeDevice;
  // The answer in hand is the current range's, for the current machine's scope. Until it lands the
  // table reads rather than drawing the previous range's rows under the new range's sentence.
  const rangeInHand =
    activityRange !== null &&
    rangeAnswer !== null &&
    rangeAnswer.range.from === activityRange.from &&
    rangeAnswer.range.to === activityRange.to &&
    rangeAnswer.device === auditScopeDevice;
  // The range is a read of its own: it is the Table's window, not the live window the poll keeps
  // fresh. A change of range or of machine scope is a new request; the same one is not repeated on
  // every poll, because its two moments are fixed and the file behind it only grows at the end.
  const readRange = useCallback(async (range: AuditRange) => {
    const device = auditWindowDevice.current;
    const generation = ++rangeReadStarted.current;
    try {
      const answer = await api.auditRange(range, device ?? undefined, AUDIT_WINDOW);
      if (generation > rangeReadApplied.current) {
        rangeReadApplied.current = generation;
        setRangeAnswer({
          range,
          device,
          lines: answer.lines,
          facts: {
            since_ms: answer.since_ms,
            bounded: answer.bounded,
            scan_limit: answer.scan_limit,
            at_limit: answer.at_limit
          }
        });
        setRangeError(false);
      }
    } catch {
      if (generation > rangeReadApplied.current) {
        rangeReadApplied.current = generation;
        setRangeAnswer(null);
        setRangeError(true);
      }
    }
  }, []);
  useEffect(() => {
    if (!session?.authenticated || activityRange === null) {
      setRangeAnswer(null);
      setRangeError(false);
      return;
    }
    setRangeError(false);
    void readRange(activityRange);
  }, [session?.authenticated, activityRange, auditScopeDevice, readRange]);
  const deviceName = (id: string) =>
    devices.find((device) => device.id === id)?.name ?? shortId(id);
  const auditReadingName = auditScopeDevice === null ? null : deviceName(auditScopeDevice);
  // The rows in hand belong to a machine the scope no longer names. The views say what is happening
  // in one line instead of drawing another window's rows under the new scope. A fleet window is not
  // another machine's: it contains every machine's newest lines, so narrowing it locally is still
  // an answer from a window that includes the scope.
  const auditReadingOther =
    !activityPaused && auditDevice !== null && auditDevice !== auditScopeDevice;
  // The scope's own read failed: the old window still cannot be drawn under the new scope, but the
  // list must not keep saying it is reading it. The page's notice states the failure.
  const auditScopeReadFailed = auditReadingOther && auditRead.state === "failed";
  // A paused list keeps its own window, so its cap sentence names the machine it was frozen from.
  const shownWindowDevice = activityPaused && frozenActivity ? frozenDevice : auditDevice;
  const shownWindowName = shownWindowDevice === null ? null : deviceName(shownWindowDevice);
  // **THE HEADER COUNTS THE DAY, NOT THE WINDOW IT IS SHOWING.** The rows are a 1,000-line slice
  // that slides; the day is `/audit/day`'s read, the same one the fleet serves as `today`. A paused
  // page counts the day it froze. A scope being replaced is not described: the old scope's day is
  // not the new one's, so the sentence is absent until the new answer arrives. The chip's subset
  // gets the hub's sentence: the day's client calls and failures, with nothing the chip has hidden.
  const shownDay = activityPaused
    ? frozenDay
    : daySummary !== null && daySummary.device === auditScopeDevice
      ? daySummary
      : null;
  const activityCount =
    shownDay !== null && !auditReadingOther
      ? activityDaySentence(shownDay.day)
      : null;
  // **THE LABELS CUT WHERE THE COUNT CUT (the forty-fourth list).** The timeline used to group its
  // rows at the BROWSER's midnight, so a reader whose zone differed from the console host's saw a
  // `Today` label over an interval the day line had not counted. The answer names its midnight; the
  // page hands it to the labels. No answer (a failed read) falls back to the browser's own day.
  const shownDayStartMs = shownDay !== null ? shownDay.day.day_start_ms ?? null : null;
  /**
   * Whether a successful window for the current scope is in hand. The day answer and the rows
   * arrive together, so their scope is the same fact; a failed reread leaves both where they were.
   * This is what tells "no window at all" from "the last window, plus a failed reread".
   */
  const auditHaveWindow =
    daySummary !== null &&
    daySummary.device === auditScopeDevice &&
    auditDevice === auditScopeDevice;
  /**
   * The page's one statement of a failed audit read, with the one Retry beside it. It names the
   * failure in the console's words and then what the page can still show; the cause is the title.
   * The shell banner no longer says the same failure again (audit-44 F3).
   */
  const auditFailedNotice =
    auditRead.state === "failed"
      ? auditHaveWindow
        ? "The console could not read the audit; the rows below are the last read, and calls that arrive live are added to them."
        : "The console could not read the audit; this timeline shows only calls that arrive live."
      : null;
  /** No window is in hand: the page's subtitle says so, and the list may not claim emptiness. */
  const auditUnread = auditRead.state !== "ready" && !auditHaveWindow;
  // A scope change already has the timeline's own "Reading <machine>'s window…" line; the page's
  // first read is the one that has no other loading surface.
  const auditReadingFirst =
    auditRead.state === "reading" && auditUnread && !auditReadingOther;

  /**
   * **THE JOIN IS ASKED, NOT GUESSED (the ledger's third item).** Each proxied call is written twice —
   * this console's audit line and the machine's own `call.started`/`call.finished|denied` pair — and
   * one `trace.call` names both. The page asks the machine about the traces this window is drawing; a
   * row whose other half the journal no longer holds is marked, and a row the console could not check
   * is marked with nothing at all.
   *
   * The scope decides what can be asked. A fleet window spans machines, so no single journal answers
   * for it and the hook asks nothing: every row is then `unknown`, which is the truth. The paused list
   * is the window it was frozen with, so it keeps asking about that one.
   */
  const join = useJoinMarks(auditScopeDevice, shownActivity);
  /**
   * **THE MACHINE'S OWN WINDOW (round 8).** The console's audit is what came through the proxy. A call
   * the machine ran on its own — the panel's, a mesh peer's, a direct loopback client's — is written
   * only in its journal, and after a reload the page drew nothing of it. The hook reads that window,
   * and the merge adds what the console does not already hold: the richer console row always wins the
   * same call, so one call is still one row. `loadOlder` pages deeper into the same journal (round
   * 10): the newest 200 rows are minutes on a busy machine, and the footer offers the rest.
   */
  const machineRead = useMachineRows(auditScopeDevice, auditReadingName);
  const machineLines = useMemo(
    () => mergeMachineRows(shownActivity, machineRead.rows),
    [shownActivity, machineRead.rows]
  );
  // The difference is exactly the machine's contribution: the merge keeps every console row and only
  // ever drops a machine row as the duplicate of one.
  const machine = machineCounts(machineRead, machineLines.length - shownActivity.length);
  /**
   * The header's line, or none. A failed read has no state sentence of its own: the banner names
   * the failure and the notice says what the page can still show, so `The audit could not be read.`
   * was the banner's sentence in another voice (audit-40 F4). A pause or a dropped-lines warning is
   * still the page's own fact and keeps its line.
   */
  const activityStatus: string | null =
    live.lagged > 0
      ? `${live.lagged} lines were dropped while this tab was behind; rereading`
      : activityPaused
        ? "Paused — the stream is still running; Resume shows what arrived."
        : auditRead.state === "failed"
          ? null
          : auditReadingFirst
            ? // **THE HEADER SAYS WHAT IS HAPPENING; THE BODY RESERVES THE SHAPE (the eightieth list).**
              // This sentence used to be the page's only loading state, drawn a second time as a paragraph
              // where the record goes. The skeleton's live label carries the read now, so the header says
              // the same thing in one word rather than the page saying it twice.
              "Reading the audit…"
            : live.state === "live"
              ? "Following the console's audit as it is written."
              : live.state === "offline"
                ? "The live stream is down; this is the last read."
                : "Opening the live stream.";

  /**
   * Record a failure by its read. A repeat replaces the read's entry in place, so an outage's many
   * polls show one banner and a new cause does not stack a second one.
   */
  const reportFailure = useCallback((failure: BannerFailure) => {
    setFailures((current) => {
      const index = current.findIndex((entry) => entry.key === failure.key);
      if (index === -1) return [...current, failure];
      const next = current.slice();
      next[index] = failure;
      return next;
    });
  }, []);

  /** The read answered: its banner has nothing left to say. Other reads' entries are untouched. */
  const clearFailure = useCallback((key: string) => {
    setFailures((current) =>
      current.some((entry) => entry.key === key)
        ? current.filter((entry) => entry.key !== key)
        : current
    );
  }, []);

  /**
   * /session is this page's first read: its failure is a subject like any other, and the login
   * card's `Try again` is its retry.
   */
  const reportProbeFailure = useCallback(
    (cause: unknown) => {
      reportFailure({
        key: "console",
        sentence: "The console could not read its own state.",
        detail: describe(cause)
      });
    },
    [reportFailure]
  );

  // The operator's session is the credential. Selecting a machine starts a session-scoped grant
  // (no envelope); `/mcp` treats an absent grant as full operator authority on that device, so every
  // workspace tab opens without a consent form. The legacy signed-grant path stays on the server for
  // existing clients; the page mints a grant only for an AI client, in Connect, where consent is the
  // point.
  useEffect(() => {
    if (selected === null) {
      setGrant(null);
      setGrantStale(false);
      return;
    }
    setGrant({ deviceId: selected, envelope: null });
    setGrantStale(false);
  }, [selected]);

  // Ask who we are first, then ask for data. Fetching the device list before knowing whether there is
  // a session earns two 401s on every cold load — visible in the browser console as failures that are
  // not failures, and exactly the noise that hides a real one later.
  const refreshData = useCallback(async () => {
    // EACH STREAM ANSWERS FOR ITSELF, AND LANDS WHEN IT ANSWERS. A failed /audit must not turn the
    // fleet into "0 devices" or Activity into "no calls": the last known values stay, and each failed
    // read keeps its own named banner. Nor may a SLOW /audit hold /fleet back: audit-38 F6 measured
    // known machines drawn as "unknown machine · <hex>" and the selected header missing because the
    // page waited for every read before drawing any of them.
    const fleetAnswer = api.devices().then(
      (list) => {
        setDevices(list);
        setFleetRead({ state: "ready", at: Date.now() });
        // The poll is the only sampler; the stored last timestamp enforces the 30 s cadence.
        // Sampling after a successful fleet fetch is what keeps an offline device from adding a
        // stale point.
        setVitals(recordVitals(list, Date.now()));
      },
      (reason) => {
        // The last list stays on screen: it is still the console's knowledge. The read state is what
        // keeps it from being drawn as an empty fleet, and the reason is what the page can say.
        // THE PAGE SAYS IT: Home and the Device picker own the one statement and the one Retry
        // (audit-44 F3); the banner does not repeat a read's failure.
        setFleetRead({ state: "failed", reason: describe(reason) });
      }
    );
    const tokensAnswer = tokensOrEmpty().then((list) => setTokens(list));
    // `rereadAudit` owns the audit read state; Activity derives its one statement and Retry from it.
    // Its rejection is already the page's fact, so this copy is answered and dropped.
    const auditAnswer = rereadAudit().catch(() => {});
    // The page's callers still await every read, so a boot or a poll settles only when all three
    // have answered; what changed is WHEN each answer reaches the page.
    await Promise.all([fleetAnswer, tokensAnswer, auditAnswer]);
  }, [rereadAudit]);

  /**
   * A scope change is a new window, so it is a new read — through the same reader the five-second
   * poll and the lag catch-up use, with the scope's device. `auditReadDevice` is the read already in
   * hand or in flight; when it already names this scope there is nothing to fetch.
   *
   * It is declared after `refreshData` because the notice's Retry is that reader: a retry answers
   * both reads and clears each banner its answer settles.
   */
  useEffect(() => {
    if (!session?.authenticated) return;
    if (auditReadDevice === auditScopeDevice) return;
    // The page's notice derives from `auditRead`; a rejected promise still needs its handler.
    rereadAudit().catch(() => {});
  }, [session?.authenticated, auditScopeDevice, auditReadDevice, rereadAudit]);

  const probe = useCallback(async () => {
    // /session decides the page; /health only labels the console's build. Waiting for both meant a
    // console whose /health failed could never render, even with a valid session.
    const state = await api.session();
    setSession(state);
    // A fresh answer to /session starts a clean page: the previous session's failures do not stand
    // over this one. The read failures `refreshData` reports below are this session's, and stay.
    setFailures([]);
    // The page remembers HOW it was let in, past a local sign-out: a proxy identity cannot be ended
    // from this tab, so the signed-out screen must not hand the operator a token form with no way to
    // use it. `via` is the console's own answer, not a guess from the headers.
    if (state.via === "proxy") setProxied(true);
    if (!state.authenticated) return;
    // A fresh session starts a fresh read: the previous session's failure must not stand over it.
    setFleetRead({ state: "reading" });
    await refreshData();
    try {
      const health = await api.health();
      setConsoleVersion(health.version);
      if (typeof health.uptime_s === "number" && Number.isFinite(health.uptime_s)) {
        setConsoleStartedAt(Date.now() - health.uptime_s * 1000);
      }
    } catch {
      /* the version label stays "…" — the page is not blocked on a cosmetic fact */
    }
  }, [refreshData]);

  const load = useCallback(async () => {
    if (!session?.authenticated) {
      await probe();
      return;
    }
    await refreshData();
  }, [probe, refreshData, session?.authenticated]);

  useEffect(() => {
    probe().catch((cause) => {
      if (cause instanceof NoAnswer || cause instanceof TypeError) {
        // The edge answered instead of the console. On this host that is an Access login page after
        // the session lapsed, and a reload is what re-enters Access; the guard is what keeps a broken
        // edge from reloading forever.
        if (reloadOnce()) {
          window.location.reload();
          return;
        }
        setSession({ authenticated: false });
        setAccessNote(
          "The console did not answer: the request was intercepted before it arrived. An expired Cloudflare Access session looks exactly like this — reload to sign in through Access."
        );
        return;
      }
      if (cause instanceof HttpError && cause.status === 401) {
        setSession({ authenticated: false });
        return;
      }
      reportProbeFailure(cause);
    });
  }, [probe, reportProbeFailure]);

  // A signed-in session polls; a signed-out one does not. The device list is the thing an operator
  // watches, and this poll is the page's only read: a manual button would repeat it and still could
  // not show its answer under a pause (C10).
  useEffect(() => {
    if (!session?.authenticated) return;
    const timer = window.setInterval(() => {
      setPollTick((current) => current + 1);
      load().catch(() => {
        /* a poll that fails is retried by the next one; the sign-in state decides the rest */
      });
    }, 5000);
    return () => window.clearInterval(timer);
  }, [session?.authenticated, load]);

  const signOut = () => {
    // Best effort, and deliberately not awaited: the request goes out with the token still in hand,
    // and the page clears whether it answers, fails or never arrives. A slow console must not leave
    // a live session in this tab.
    // The console's cookie is ours; behind Access the identity is the edge's. When the answer names
    // the edge's logout, follow it — otherwise the next request is signed straight back in and the
    // button looks broken.
    api
      .logout()
      .then((answer) => {
        if (answer?.access_logout) window.location.assign(answer.access_logout);
      })
      .catch(() => {});
    setSession({ authenticated: false });
    setFailures([]);
    setDevices([]);
    setFleetRead({ state: "reading" });
    setVitals({});
    setAudit([]);
    // A signed-out page has no audit answer, and the next session's first read must not be mistaken
    // for a poll over this one's.
    setAuditRead({ state: "reading" });
    auditAnsweredDevice.current = undefined;
    setTokens([]);
    setActivityRange(null);
    setRangeAnswer(null);
    setRangeError(false);
    setSelected(null);
    storeSelectedDevice(null);
    setRunsFocus(null);
    setGrant(null);
    setGrantStale(false);
    setPage("home");
    setTab("terminal");
    setOpenTool(null);
    setConnect(null);
    setCode(null);
    // THE WAY OUT LEAVES THE PANEL'S ADDRESS. The signed-out card is a page with a URL, not a state
    // a reader can only reach by pressing this button (audit-39 F6).
    setSignInRoute(true);
    window.history.replaceState(null, "", SIGN_IN_HASH);
  };

  /** Pick a machine (or All machines) and remember the choice, staying on the current destination. */
  const choose = (id: string | null) => {
    setSelected(id);
    storeSelectedDevice(id);
  };

  const go = (next: Page) => {
    setPage(next);
    if (next === "device" && page !== "device") setTab("terminal");
    setOpenTool(null);
  };

  const openRun = (deviceId: string, runId: string) => {
    window.history.replaceState(null, "", runLink(deviceId, runId));
    setSelected(deviceId);
    storeSelectedDevice(deviceId);
    setRunsFocus(runId);
    setPage("device");
    setTab("runs");
  };

  // The fragment is the page's address: a reload keeps the destination and the Device tab, and a run
  // link keeps both the machine and the run. Leaving Runs replaces the run link with the new surface
  // instead of leaving a stale run in the URL.
  useEffect(() => {
    if (!session?.authenticated) return;
    // `#signin` is a view the address asks for, not a surface this effect may overwrite with #home.
    if (signInRoute) return;
    const run = parseRunLink(window.location.hash);
    // **THE RUN LINK OUTLIVES THE FOCUS IT CARRIED.** The guard used to require `runsFocus`, which
    // RunsPage clears the moment it has opened the record (`onFocusConsumed`). Whichever settled
    // second decided the address: if the consume won, this effect saw no focus and rewrote
    // `#run/<device>/<run>` to `#device/runs`, so a run link did not survive a reload — it survived
    // it only when the effect happened to run first. Measured as the round's one flaky test, it is
    // the same defect the flake was pointing at: the address a run link names is the run, not the
    // tab. While the page IS the run link's own surface, this effect leaves the address alone.
    if (run && page === "device" && tab === "runs" && run.deviceId === selected) return;
    const wanted = surfaceHash({
      page,
      tab: page === "device" ? tab : undefined,
      tool: page === "device" && tab === "tools" ? openTool ?? undefined : undefined,
      view: page === "device" && tab === "screen" ? screenView : undefined,
      // The Table's range is the page's address while it is chosen, so the same link reopens the
      // same request. Clearing it puts `#activity` back.
      range: page === "activity" ? activityRange ?? undefined : undefined
    });
    if (window.location.hash !== wanted) window.history.replaceState(null, "", wanted);
  }, [page, tab, screenView, openTool, runsFocus, session?.authenticated, activityRange, signInRoute]);

  // A page or a machine change replaces the control the operator pressed: a rail click left focus on
  // the rail, and a fleet row's door unmounted its own button. The new page's <main> takes the focus,
  // so the heading and its first control are one Tab away instead of a traversal from the top. The
  // first render is exempt: sign-in keeps the focus the browser gave its own form.
  useEffect(() => {
    if (!mounted.current) {
      mounted.current = true;
      return;
    }
    mainRef.current?.focus();
  }, [page, selected]);

  // **THE ADDRESS BAR IS PART OF THE SURFACE, NOT ONLY A RECORD OF IT.** Every navigation wrote the
  // fragment and nothing read it back: Back and Forward moved the URL and left the view where it was,
  // and a link pasted into a running tab did nothing until a reload. The same parsers the first render
  // uses apply every fragment change, so a hash the app cannot read leaves the screen alone.
  useEffect(() => {
    const apply = () => {
      // THE SIGN-IN PANEL IS AN ADDRESS, NOT A GESTURE'S SIDE EFFECT. Reachable signed in or out,
      // and never rewritten to #home by the surface effect while the hash names it.
      if (window.location.hash === SIGN_IN_HASH) {
        setSignInRoute(true);
        return;
      }
      setSignInRoute(false);
      const run = parseRunLink(window.location.hash);
      if (run) {
        setSelected(run.deviceId);
        storeSelectedDevice(run.deviceId);
        setRunsFocus(run.runId);
        setPage("device");
        setTab("runs");
        return;
      }
      const surface = parseSurfaceHash(window.location.hash, {
        machineSelected: selected !== null
      });
      if (!surface) return;
      setRunsFocus(null);
      setPage(surface.page);
      // A fragment that names a range restores that request; one that does not puts the newest
      // window back. A range link can only be shown by the Table.
      setActivityRange(surface.range ?? null);
      if (surface.range) setActivityView("table");
      if (surface.page === "device") {
        if (surface.tab) setTab(surface.tab);
        if (surface.tab === "screen" && surface.view) setScreenView(surface.view);
        setOpenTool(surface.tab === "tools" ? surface.tool ?? null : null);
      } else {
        setOpenTool(null);
      }
    };
    window.addEventListener("hashchange", apply);
    return () => window.removeEventListener("hashchange", apply);
  }, [selected]);

  // The tab is part of the surface: a window full of consoles should say which one this is, and
  // the signed-out tab should not claim a machine page it is not showing.
  useEffect(() => {
    // **ONE PAGE, ONE NAME (audit-45 F4).** Home's h1 said `Machines` while the rail that opened it
    // said `Home` and the tab title repeated the h1: the same destination under three names. The
    // page is the fleet's verdict and next action, not a table, so its name is the rail's, and the
    // title reads it from the same map. Device without a machine is the picker, and the tab title
    // says that rather than a tab that is not drawn (audit-40 F7).
    const current =
      page === "device"
        ? chosen === null
          ? "Choose a machine"
          : DEVICE_TAB_LABELS[tab]
        : PAGE_LABELS[page];
    // **THE TITLE FOLLOWS THE PANEL.** `#signin` while the tab is signed in draws `You are signed
    // in`, and the title said `Sign in · Summrise console`: a tab in a row of tabs named the
    // opposite of its page. The signed-in panel gets its own title; every other state keeps the
    // one the destination names, and a signed-out panel keeps `Sign in`.
    document.title = !session?.authenticated
      ? "Sign in · Summrise console"
      : signInRoute
        ? "Signed in · Summrise console"
        : `${current} · Summrise console`;
  }, [page, tab, chosen, session?.authenticated, signInRoute]);

  const signIn = async () => {
    setBusy(true);
    try {
      // The console answers with Set-Cookie; the page keeps nothing. `load` re-probes `/session`,
      // which now carries the cookie, so the fleet appears without a token in this tab.
      await api.signIn({ token: tokenInput.trim() });
      setTokenInput("");
      // A successful sign-in leaves the panel: `#signin` is its address, not a place to stay after
      // the credential is accepted.
      setSignInRoute(false);
      if (session?.authenticated) await probe();
      else await load();
    } catch (cause) {
      // THE REFUSAL'S OWN SENTENCE IS THE ACTION. The Rust `/auth` 401 names the value to type and
      // the flag whose file holds it; the generic apology is only for a refusal that said nothing.
      // `HttpError.message` falls back to the machine code, so a bare `unauthorized` is not a
      // sentence: only a message distinct from the code is the console's own words. The console
      // writes a fragment; on the page it is a sentence, so it starts with a capital (audit-39 F6).
      const refusal =
        cause instanceof HttpError && cause.message && cause.message !== cause.code
          ? asSentence(cause.message)
          : null;
      reportFailure({
        key: "sign-in",
        sentence: refusal ?? "The console could not sign you in.",
        detail: describe(cause)
      });
    } finally {
      setBusy(false);
    }
  };

  const applyRename = async () => {
    if (!renaming) return;
    clearFailure("rename");
    const name = draftName.trim();
    try {
      await api.rename(renaming.id, name === "" ? null : name);
      setRenaming(null);
      setNotice(name === "" ? "Name cleared; the device's own hostname applies." : `Renamed to ${name}.`);
      await load();
    } catch (cause) {
      reportFailure({
        key: "rename",
        sentence: "The console could not rename the machine.",
        detail: describe(cause)
      });
    }
  };

  const applyRemove = async () => {
    if (!removing) return;
    clearFailure("remove");
    try {
      await api.remove(removing.id);
      setNotice(`${removing.name ?? shortId(removing.id)} was removed; its channel closed.`);
      setRemoving(null);
      if (selected === removing.id) choose(null);
      await load();
    } catch (cause) {
      reportFailure({
        key: "remove",
        sentence: "The console could not remove the machine.",
        detail: describe(cause)
      });
    }
  };

  // Remote troubleshooting has the same problem as the panel's: a screenshot is not a bug report. The
  // console can assemble the same facts it can see, and say which of them it can see.
  const copyDiagnostics = async (device: Device | null) => {
    if (!device) return;
    // The row's age and this line read the same evidence: a diagnostic that says "never" while the
    // fleet row says "last seen 1h 49m ago" is the same bug at a different surface.
    const heard = lastHeardMs(device);
    const lines = [
      "Summrise device diagnostics (from the console)",
      `device: ${device.name ?? "(unnamed)"} (${device.id})`,
      `online: ${device.online ? "yes" : "no"}  version: ${device.version ?? "unknown"}  last heard: ${heard !== null ? new Date(heard).toISOString() : "never"}`,
      device.last_attempt && device.last_attempt.attempts > 0
        ? `console link: ${retrySentence(device.last_attempt.attempts, device.last_attempt.reason)}`
        : "console link: no failure reported since it last connected",
      "recent audit for this device:"
    ];
    // THE PAGE'S WINDOW MAY BE ONE MACHINE'S. Diagnostics describe the machine they name, not
    // whatever window Activity is showing; another machine gets its own short read instead of a
    // "none" claim about a window that never contained it.
    let recent: AuditLine[] | null = auditDevice === device.id ? audit : null;
    if (recent === null) {
      recent = await api.audit(8, device.id).catch(() => null);
    }
    if (recent === null) {
      lines.push("  (the console could not read this machine's audit)");
    } else {
      const mine = recent.filter((line) => line.device_id === device.id).slice(0, 8);
      if (mine.length === 0) lines.push(`  (none in the newest ${recent.length} lines)`);
      for (const line of mine) {
        lines.push(
          `  ${new Date(line.ts_ms).toISOString()}  ${line.kind} ${line.tool ?? line.detail ?? ""} ${line.status}`.trimEnd()
        );
      }
    }
    const text = lines.join("\n");
    if (!(await copyText(text))) {
      setNotice("Could not copy the diagnostics: the browser refused the clipboard.");
      return;
    }
    setNotice("Diagnostics copied for " + (device.name ?? shortId(device.id)) + ".");
  };

  // One answer at a time: a second press before the first answers would mint a second, unseen
  // one-time code. `enrolling` draws the button's state; this ref is the guard the state cannot be
  // until React redraws.
  const mintInFlight = useRef(false);
  const mintCode = async () => {
    if (mintInFlight.current) return;
    mintInFlight.current = true;
    clearFailure("enroll");
    setEnrolling(true);
    try {
      setCode(await api.enroll());
    } catch (cause) {
      reportFailure({
        key: "enroll",
        sentence: "The console could not create an enrollment code.",
        detail: describe(cause)
      });
    } finally {
      mintInFlight.current = false;
      setEnrolling(false);
    }
  };

  /// Add a machine is one verb: Home owns the fleet, the code panel is part of it.
  ///
  /// **THE CODE IS THE DESTINATION.** The selected machine's hub stays on screen otherwise, and a
  /// press whose whole purpose is to produce a command looks like a dead one while the minted code
  /// waits on a page nobody was sent to. Clearing the choice is also the address: All machines is
  /// where the fleet panel lives. A live code already in hand is shown again rather than silently
  /// minting a second one-time code; Dismiss forgets it, and the next press mints a fresh one.
  const addMachine = () => {
    setPage("home");
    choose(null);
    clearFailure("enroll");
    const live = code !== null && code.expires_at_ms > Date.now();
    if (!live) void mintCode();
  };

  if (session === null) {
    return (
      <div className="login-wrap">
        <section className="panel login">
          <img src={logoUrl} alt="" width={40} height={40} />
          <h1>Connecting to the console</h1>
          {failures.length > 0 ? (
            <>
              <p className="error" title={failures[0].detail}>
                {failures[0].sentence}
              </p>
              <button
                type="button"
                className="primary"
                onClick={() => void probe().catch(reportProbeFailure)}
              >
                Try again
              </button>
            </>
          ) : (
            <p className="hint">Reading the console's own state…</p>
          )}
        </section>
      </div>
    );
  }

  // THE PANEL HAS AN ADDRESS. `#signin` draws it whether or not this tab holds a session, so a
  // person told "the sign-in page" has somewhere to go without first signing out (audit-39 F6).
  if (!session.authenticated || signInRoute) {
    const alreadySignedIn = session.authenticated;
    /**
     * The token form, wherever the panel offers it: the signed-out card's way in, the proxied
     * card's alternative, and — while signed in — the disclosed way to replace the session.
     */
    const tokenForm = (
      <>
        <label htmlFor="token">Operator token</label>
        <input
          id="token"
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={tokenInput}
          onChange={(event) => setTokenInput(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && tokenInput.trim()) void signIn();
          }}
          placeholder="64 hex characters"
        />
        <button className="primary" type="button" disabled={busy || tokenInput.trim().length === 0} onClick={() => void signIn()}>
          {busy ? "Checking…" : "Sign in"}
        </button>
      </>
    );
    return (
      <div className="login-wrap">
        <section className="panel login">
          <img src={logoUrl} alt="" width={40} height={40} />
          {alreadySignedIn ? (
            <>
              {/* **THE TITLE AGREES WITH THE SENTENCE, AND THE SAFE ACTION IS FIRST.** The panel used
                  to be headed `Sign in to the console` while its sentence said this tab was signed
                  in, with the credential-replacing form as the primary. Now the page names the
                  state, Back leads, and the form is one disclosure away (audit-40 F8). */}
              <h1>You are signed in</h1>
              <p className="hint">
                This tab is already signed in. Back returns to the console without changing that;
                a different token replaces the sign-in only when the console accepts it.
              </p>
              <div className="login-actions">
                <button
                  type="button"
                  className="primary"
                  onClick={() => setSignInRoute(false)}
                >
                  Back to the console
                </button>
              </div>
              <details className="token-disclosure">
                <summary>Use a different token</summary>
                {tokenForm}
              </details>
            </>
          ) : proxied ? (
            <>
              {/* SIGNING OUT IS A TAB'S GESTURE, NOT THE IDENTITY'S. The console was reached through
                  a proxy that stamps the tailnet login on every request, so a local sign-out cannot
                  end it — and the token form below, shown alone, would be a dead end for a person
                  who was never given a token. Say what happened and how to come back. */}
              <h1>Signed out of this tab</h1>
              <p className="hint">
                This console signs operators in through the network: the proxy in front of it sends
                your tailnet login with every request. Signing out cleared this tab, and loading it
                again signs you back in.
              </p>
              <div className="login-actions">
                <button
                  type="button"
                  className="primary"
                  // Drop `#signin` before reloading: the address is the panel, and a proxied identity
                  // that reloads there would be shown the panel again rather than the console.
                  onClick={() =>
                    window.location.assign(window.location.pathname + window.location.search)
                  }
                >
                  Load the console again
                </button>
              </div>
              <p className="hint">Or paste the operator token below.</p>
            </>
          ) : (
            <>
              <h1>Sign in to the console</h1>
              <p className="hint">
                The operator token is the 64-character secret in the file the console's{" "}
                <code>--operator-token-file</code> names, on the console host.
              </p>
              {accessNote && <p className="error">{accessNote}</p>}
              {!accessNote && session.access?.header && session.access.error && (
                <p className="error">
                  Cloudflare Access signed you in, but the console refused the token:{" "}
                  {session.access.error}
                </p>
              )}
              {!accessNote && !session.access?.header && session.access?.configured && (
                <p className="hint">
                  This page has no sign-in. Reload through Cloudflare Access, or paste the operator
                  token below.
                </p>
              )}
              {!accessNote && session.access && !session.access.configured && (
                <p className="hint">
                  Access trust is off on this console, so the operator token is the only way in.
                </p>
              )}
            </>
          )}
          {!alreadySignedIn && tokenForm}
          {failures.length > 0 && (
            // A REFUSED SIGN-IN IS AN ALERT. The shell's banner already announces other failures;
            // the panel's sentence is the console's answer to the press and is announced the same
            // way (audit-39 F6), with the browser's cause in the title.
            <p
              className="error"
              role="alert"
              title={
                failures
                  .map((failure) => failure.detail)
                  .filter(Boolean)
                  .join(" · ") || undefined
              }
            >
              {failures.map((failure) => failure.sentence).join(" ")}
            </p>
          )}
        </section>
      </div>
    );
  }

  // THE COMMAND THE CONSOLE PRINTS IS THE ONE THE MANUAL PROVES. Without --data the code is spent
  // into a store beside the exe, and the installed SummriseLaunch service never sees it; without
  // .exe an elevated Command Prompt looks for a file name Windows does not resolve.
  const enrollCommand = code
    ? `summrise-device.exe --enroll ${location.origin} ${code.code} --data C:\\ProgramData\\Summrise`
    : "";

  const account = (
    <AccountMenu
      session={session}
      consoleVersion={consoleVersion}
      onSignOut={signOut}
      onConnect={() => setConnect({ section: "endpoint", device: chosen })}
      onTokens={() => setConnect({ section: "tokens", device: chosen })}
      onNotice={setNotice}
    />
  );

  return (
    <div className="shell">
      <a className="skip-link" href="#main">Skip to content</a>
      <header className="topbar">
        <div className="topbar-inner">
          <button
            type="button"
            className="brand"
            aria-label="Summrise home"
            onClick={() => setPage("home")}
          >
            <img src={logoUrl} alt="" width={24} height={24} />
            <span className="brand-name">Summrise</span>
          </button>
          <MachineMenu
            devices={devices}
            chosen={chosen}
            onChoose={choose}
            onAddMachine={addMachine}
          />
          <div className="topbar-actions">{phone && account}</div>
        </div>
      </header>

      {/* **THE STALE TAB'S NOTICE STANDS ON ITS OWN (the hundred-and-ninety-ninth list).** It was written inside the failure stack,
          which is drawn only when an action has failed — so the one page that most needed telling (a quiet tab from before a deploy)
          could never see it. Measured: a page stamped `-ux200` against a console serving `-ux253` sat for eighty seconds with nothing. */}
      {newerBuild !== null && (
        <div className="alert-stack">
          <div className="alert-banner alert-stale" role="status">
            <span>
              A newer console is deployed ({newerBuild}); this page is {ownBuild}. Reload to use it.
            </span>
            <button type="button" onClick={() => window.location.reload()}>
              Reload
            </button>
          </div>
        </div>
      )}

      {failures.length > 0 && (
        // **BELOW THE TOPBAR, IN THE SHELL'S OWN ROW (audit-44 F3).** The stack used to float at
        // the window's top and covered the machine switcher exactly when the console was reporting
        // a failure. It now occupies the `alerts` grid row between the header and the page, so the
        // cost of an action's refusal is a strip of height, not the machine's name.
        //
        // POST-SIGN-IN FAILURES HAD NO SURFACE: every handler wrote an error string, and only the
        // signed-out card ever drew it. The banner keeps each action's failure its own and keeps
        // the technical cause in the title (audit-38 F2). A repeat of the same action replaces its
        // entry instead of stacking another banner.
        <div className="alert-stack">
          {failures.map((failure) => (
            <div className="alert-banner" role="alert" key={failure.key}>
              <span title={failure.detail}>{failure.sentence}</span>
              <button type="button" onClick={() => clearFailure(failure.key)}>
                Dismiss
              </button>
            </div>
          ))}
        </div>
      )}

      <Rail page={page} onPage={go} footer={phone ? undefined : account} />

      <main
        id="main"
        ref={mainRef}
        tabIndex={-1}
        className={page === "device" && tab === "terminal" ? "terminal-main" : undefined}
      >
        {page === "home" && (
          <HomePage
            device={chosen}
            devices={devices}
            fleetRead={fleetRead}
            onRetryFleet={() => void load().catch(() => {})}
            selectedId={selected}
            code={code}
            enrollCommand={enrollCommand}
            busy={enrolling}
            // **THE DOOR OPENS THE MACHINE.** A fleet row's visible name button used to call
            // `choose` and leave the page on `#home`, so the operator who pressed `Open
            // DESKTOP-…` stayed where they were (audit-43 F1). It now selects and opens Device →
            // Overview; the attention row opens Device → Machine, where an absent machine's last
            // facts live. The topbar menu and the picker stay choosers.
            onOpen={(id) => {
              choose(id);
              setPage("device");
              setTab("overview");
            }}
            onOpenMachine={(id) => {
              choose(id);
              setPage("device");
              setTab("machine");
            }}
            onOpenTab={(id, tab) => {
              choose(id);
              setPage("device");
              setTab(tab);
            }}
            liveLines={live.lines}
            liveState={live.state}
            liveRunning={live.running}
            onFailedMachine={(deviceId) => {
              // The row's count and the page's filter are the same two facts: this machine, the failures.
              setActivityFilter((current) => ({ ...current, device: deviceId, status: "failed" }));
              setPage("activity");
            }}
            onFailedToday={() => {
              // **THE DAY'S FAILURES, ONE CLICK (the two-hundredth list).** Activity's own filters, set the way the number was counted.
              setActivityFilter((current) => ({ ...current, status: "failed" }));
              setPage("activity");
            }}
            onNow={(call) => {
              // **A FINISHED RUN'S LINE OPENS THE RUN (the hundred-and-ninety-sixth list).** `runs.exec` names its run in the detail, and
              // the run's own result is what a person wants next — the Device page's Runs tab already focuses a run by id.
              const finished = call.tool === "runs.exec" ? /^run (\S+) /.exec(call.detail ?? "") : null;
              if (finished !== null && call.deviceId !== null) {
                openRun(call.deviceId, finished[1]);
                return;
              }
              // Otherwise Activity opens scoped to the call: its tool and its machine, both of which the page's filter already carries.
              setActivityFilter((current) => ({ ...current, tool: call.tool, device: call.deviceId ?? "" }));
              setPage("activity");
            }}
            onRename={(device) => {
              setRenaming(device);
              setDraftName(device.name ?? "");
            }}
            onRemove={setRemoving}
            onDiagnostics={(device) => void copyDiagnostics(device)}
            onAddMachine={addMachine}
            onTokens={() => setConnect({ section: "tokens", device: chosen })}
            onDismissCode={() => setCode(null)}
            onNotice={setNotice}
            onConnect={() => setConnect({ section: "endpoint", device: chosen })}
            onDevice={() => {
              setPage("device");
              setTab("overview");
            }}
            onTools={() => {
              setPage("device");
              setTab("tools");
            }}
          />
        )}

        {page === "activity" && (
          <div className="page activity-page">
            <header className="page-head">
              <div>
                <h1>Activity</h1>
                {/* The day at a glance, from the same rows the timeline draws. It is not a live
                    region: it moves with the whole window, and re-announcing every line would make
                    the stream a conversation. */}
                {/* **THE HEADER'S DAY IS NOT A RANGE'S COUNT.** `/audit/day` counts today from
                    the window's own read; while the Table shows a chosen range, that sentence is
                    about another interval, and the table's own sentence is the one that names the
                    rows on screen. */}
                {/* **THE DAY AND THE WINDOW ARE TWO INTERVALS, AND THE PAGE SAYS SO ONCE (the
                    forty-third list).** Measured live at 19:48: this line read `Today: 206 client
                    calls, 21 failed` and the bar under it read `629 client calls in this window ·
                    spanning 12 h 15 m`. Both true, and nothing said that the window STARTS AT 07:33
                    — so a reader comparing them can only conclude one is wrong. The day's own
                    sentence says its interval (it always did: `Today`), the window's says its start,
                    and this one clause says how they relate. It is drawn only when both numbers are
                    on the page, because a sentence describing a comparison nobody can see is noise. */}
                {/* **THE INTERVAL IS DATA, NOT AN ADJECTIVE (the forty-fifth list).** `Today` is a
                    word; a gate can only match it with a regular expression, and three lists in a row
                    ended in a comparison between two intervals because of exactly that. The element
                    now carries the interval it counts — `data-interval-kind` and the two moments, in
                    Unix milliseconds — so a gate compares NUMBERS with the interval the window's
                    count carries, instead of comparing two English sentences. */}
                {activityCount !== null && activityRange === null && (
                  <p
                    /* **THE COUNT IS DATA, NOT THE SUBTITLE (the hundred-and-seventh list).** It wore
                       `.page-sub` — the atom's ONE subtitle slot — so `head.maxHeadsPerPage: 1` could not see
                       that this page put TWO lines in one slot: the day's count and the stream's status.
                       A count is a fact of the same kind the verdict sentence is on Home; the subtitle is
                       the page's context. The two are different elements now. */
                    className="activity-today"
                    data-interval-kind="day"
                    data-interval-from={shownDayStartMs ?? undefined}
                    data-interval-to={activityPaused && pausedAt !== null ? pausedAt : undefined}
                  >
                    {activityCount}
                  </p>
                )}
                {/* **THE FAILED READ HAS ONE SENTENCE, AND IT IS THE BANNER'S.** This line is
                    the page's state (following, reading, paused, lagging) and is absent when the
                    only thing to say would be the failure again (audit-40 F4). */}
                {failureTools.length > 0 && (
                  // **THE NUMBER GETS A PLACE (the hundred-and-eighty-sixth list).** Each chip narrows to that tool AND to the
                  // failures, so the sentence stops being a fact with no door.
                  <p className="page-sub activity-failures">
                    <span>Failures by tool:</span>
                    {failureTools.map(([tool, count]) => (
                      <button
                        key={tool}
                        type="button"
                        className="link"
                        onClick={() => {
                          setActivityFilter((current) => ({ ...current, tool, status: "failed" }));
                        }}
                      >
                        {tool} · {count}
                      </button>
                    ))}
                  </p>
                )}
                {activityStatus !== null && (
                  <p className="page-sub activity-status">{activityStatus}</p>
                )}
              </div>
              <div className="page-actions">
                  <div className="seg stream-views" role="tablist" aria-label="Activity view">
                    <button
                      type="button"
                      role="tab"
                      aria-selected={activityView === "stream"}
                      className={activityView === "stream" ? "seg-on" : undefined}
                      onClick={() => setActivityView("stream")}
                    >
                      Timeline
                    </button>
                    <button
                      type="button"
                      role="tab"
                      aria-selected={activityView === "table"}
                      className={activityView === "table" ? "seg-on" : undefined}
                      onClick={() => setActivityView("table")}
                    >
                      Table
                    </button>
                  </div>
                <span className={"live-state" + (live.state === "live" && !activityPaused ? " is-active" : "")}>
                  <span className="live-dot" aria-hidden="true" />
                  {activityPaused ? "Paused" : live.state === "live" ? "Live" : live.state === "offline" ? "Offline" : "Connecting"}
                </span>
                <button type="button" onClick={pauseActivity}>
                  {activityPaused ? (freshActivity > 0 ? `Resume (${freshActivity} new)` : "Resume") : "Pause"}
                </button>
              </div>
            </header>
            {auditFailedNotice !== null && (
              // THE PAGE'S ONE STATEMENT AND ONE RETRY. The failure is named here, in the console's
              // words with the technical cause in the title; the shell banner used to say it again
              // over the topbar (audit-44 F3).
              <div className="notice" role="status">
                <span title={auditRead.state === "failed" ? auditRead.reason : undefined}>
                  {auditFailedNotice}
                </span>
                <button
                  type="button"
                  onClick={() => {
                    setAuditRead({ state: "reading" });
                    void refreshData().catch(() => {});
                  }}
                >
                  Retry
                </button>
              </div>
            )}
            {selectedGone && selected !== null && (
              // HOME AND DEVICE FALL BACK; ACTIVITY MUST NOT KEEP FILTERING. The id is not a
              // machine (a stale bookmark, or one the fleet row's Remove ended), so the page draws
              // every machine and says the selection is gone instead of naming it in hex.
              <div className="notice" role="status">
                <span>
                  {shortId(selected)} is not in this console's fleet. The timeline shows every
                  machine; its past activity is still searchable.
                </span>
                <button type="button" onClick={() => choose(null)}>
                  Clear the selection
                </button>
              </div>
            )}
            <section className={activityView === "stream" ? "panel activity-panel is-record" : "panel activity-panel"}>
              {/* **THE VIEW SWITCH LIVES IN THE HEAD, NOT ON A ROW OF ITS OWN (the hundred-and-sixtieth list).** Measured at
                  1440: the segmented control stood alone in a 40 px bar above the filters, so the record began at y=369 —
                  the page's own switch is a page-level control and belongs beside Live/Pause, where the head already keeps
                  the page's actions. The bar survives only for the table view's sentence. */}
              {activityView === "table" && (
                <div className="stream-bar">
                  <p className="hint stream-note">
                    The audit as rows, with filters and CSV or JSON Lines export.
                  </p>
                </div>
              )}
              {activityView === "stream" ? (
                <ActivityTimeline
                  lines={machineLines}
                  dayStartMs={shownDayStartMs}
                  devices={devices}
                  tokens={tokens}
                  capped={shownCapped}
                  atLimit={shownAtLimit}
                  cappedMachine={shownWindowName}
                  windowCounts={shownCounts}
                  // **THE FIRST READ IS A READ TOO (the eightieth list).** `auditReadingOther` is the scope's
                  // SECOND read — a changed chip asking for another machine's window — and the FIRST read
                  // was left to a sentence in the header alone, which is why the page drew no shape at all
                  // while the fleet's window was on its way. `auditReadingFirst` is that state, and the
                  // skeleton's label names which window is being read.
                  loading={auditReadingOther || auditReadingFirst}
                  loadingFailed={auditScopeReadFailed}
                  loadingMachine={auditReadingName}
                  unread={auditUnread}
                  source={auditSource}
                  join={join}
                  machine={machine}
                  onMachineOlder={machineRead.loadOlder}
                  query={activityFilter.tool}
                  onQuery={(tool) => setActivityFilter((current) => ({ ...current, tool }))}
                  clientsOnly={activityClientsOnly}
                  onClientsOnly={setActivityClientsOnly}
                  status={activityFilter.status}
                  onStatus={(status) => setActivityFilter((current) => ({ ...current, status }))}
                  onFilterTool={(tool) => setActivityFilter((current) => ({ ...current, tool }))}
                  device={activityMachine}
                  // THE FALLBACK'S ID IS NOT A SCOPE. Its rows are still in the window (the page
                  // says the past activity stays searchable), but the timeline must not turn the
                  // id into an `unknown machine · <hex>` chip above them (audit-38 F8).
                  goneId={selectedGone ? selected : null}
                  onDevice={(deviceId) => {
                    setActivityFilter((current) => ({ ...current, device: deviceId }));
                    // A chip is the scope: the header's machine is what the table calls "This
                    // machine"; another machine is the table's device filter, and either way the
                    // read above takes its window from this same device.
                    setActivityScope(
                      deviceId !== "" && deviceId === activeDevice ? "machine" : "all"
                    );
                  }}
                  nowMs={activityPaused && pausedAt !== null ? pausedAt : undefined}
                  onWatch={(deviceId) => {
                    choose(deviceId);
                    setPage("device");
                    setTab("terminal");
                  }}
                  onManageToken={(tokenId) =>
                    setConnect({ section: "tokens", device: chosen, tokenId })
                  }
                />
              ) : (
                <AuditTable
                  // A range's rows are the range's answer; the newest window (and the live lines
                  // merged into it) is what the page draws until a range is chosen.
                  lines={activityRange === null ? shownActivity : rangeInHand ? rangeAnswer.lines : []}
                  capped={activityRange === null && shownCapped}
                  atLimit={activityRange === null && shownAtLimit}
                  cappedMachine={shownWindowName}
                  windowCounts={shownTableCounts}
                  loading={
                    activityRange === null ? auditReadingOther : !rangeInHand && !rangeError
                  }
                  loadingFailed={activityRange === null && auditScopeReadFailed}
                  loadingMachine={auditReadingName}
                  unread={auditUnread}
                  source={auditSource}
                  join={auditScopeDevice === null ? null : join}
                  devices={devices}
                  tokens={tokens}
                  selectedDeviceId={activeDevice}
                  onOpenRun={openRun}
                  onManageToken={(tokenId) =>
                    setConnect({ section: "tokens", device: chosen, tokenId })
                  }
                  filter={activityFilter}
                  onFilter={setActivityFilter}
                  range={activityRange}
                  onRange={setActivityRange}
                  rangeFacts={rangeInHand ? rangeAnswer.facts : null}
                  rangeError={activityRange !== null && rangeError}
                  scope={activityScopeNow}
                  onScope={setActivityScope}
                />
              )}
            </section>
          </div>
        )}

        {page === "device" && (
          <DevicePageShell
            tab={tab}
            onTab={(next) => {
              setOpenTool(null);
              setTab(next);
            }}
            devices={devices}
            chosen={chosen}
            onChoose={(id) => choose(id)}
            onConnect={() => setConnect({ section: "endpoint", device: chosen })}
            fleetRead={fleetRead}
            onRetryFleet={() => void load().catch(() => {})}
            journalLines={deviceJournal(live.lines, chosen?.id)}
          >
            {() => (
              <>
                {grantStale && chosen && (
                  <section className="panel empty">
                    <h2>The console refused this session</h2>
                    <p className="hint">
                      The console no longer accepts this session's calls. Reconnect to start a fresh
                      one; the open tabs keep their place.
                    </p>
                    <button
                      type="button"
                      onClick={() => {
                        setGrant({ deviceId: chosen.id, envelope: null });
                        setGrantStale(false);
                      }}
                    >
                      Reconnect
                    </button>
                  </section>
                )}
                {tab === "overview" && chosen ? (
                  <OverviewPage
                    device={chosen}
                    onTools={() => setTab("tools")}
                    onActivity={() => setPage("activity")}
                    onTab={(next) => setTab(next)}
                    liveLines={live.lines}
                    liveRunning={live.running}
                    liveState={live.state}
                    tick={pollTick}
                    liveRecoveries={live.recoveries}
                    consoleStartedAt={consoleStartedAt}
                  />
                ) : null}
                {tab === "screen" && grant ? (
                  <section className="panel">
                    <ScreenPage
                      grant={grant}
                      deviceId={chosen?.id ?? null}
                      view={screenView}
                      onViewChange={setScreenView}
                      deviceOnline={chosen?.online}
                      helperRunning={chosen?.host?.helper?.registered}
                      helperSession={chosen?.host?.helper?.session}
                      sessions={chosen?.host?.sessions}
                    />
                  </section>
                ) : null}
                {tab === "files" && grant ? (
                  <section className="panel">
                    <FilesPage grant={grant} deviceOnline={chosen?.online} />
                  </section>
                ) : null}
                {tab === "runs" && grant ? (
                  <section className="panel">
                    <RunsPage
                      grant={grant}
                      focusRunId={runsFocus}
                      onFocusConsumed={() => setRunsFocus(null)}
                      deviceOnline={chosen?.online}
                    />
                  </section>
                ) : null}
                {tab === "tools" && grant && chosen ? (
                  <ToolsTab
                    device={chosen}
                    deviceOnline={chosen.online}
                    grant={grant}
                    tool={openTool}
                    onTool={setOpenTool}
                    onMachine={() => {
                      setOpenTool(null);
                      setTab("machine");
                    }}
                    onBrowser={() => {
                      setOpenTool(null);
                      setTab("screen");
                      setScreenView("browser");
                    }}
                  />
                ) : null}
                {tab === "memory" && grant ? (
                  <section className="tab-body">
                    <MemoryPage grant={grant} deviceOnline={chosen?.online} />
                  </section>
                ) : null}
                {tab === "machine" && grant ? (
                  <section className="tab-body">
                    <SystemPage
                      grant={grant}
                      device={chosen}
                      deviceOnline={chosen?.online}
                      consoleStartedAt={consoleStartedAt}
                      /* **ONE MACHINE HAS ONE CURRENCY (the thirty-fourth list).** The machine's
                         page computes its target with the same helper Home's attention block calls,
                         from the same fleet and the same installer read, so a machine Home calls
                         behind cannot call itself current on its own Machine tab. */
                      fleet={devices}
                      vitals={chosen ? vitals[chosen.id] ?? [] : []}
                    />
                  </section>
                ) : null}
              </>
            )}
          </DevicePageShell>
        )}

        {/*
          The terminal stays mounted whichever destination is showing — a rail click used to destroy
          its tabs, cursors and xterms. It is hidden rather than unmounted, and the page wrapper is
          what gives it the destination's width and padding when it is visible.
        */}
        {chosen && grant && grant.deviceId === chosen.id && (
          <div
            className="page terminal-page"
            hidden={!(page === "device" && tab === "terminal")}
          >
            <section className="panel terminal-panel">
              <TerminalWorkspace
                grant={grant}
                visible={page === "device" && tab === "terminal"}
                deviceOnline={chosen.online}
                lastSeenMs={lastHeardMs(chosen)}
                deviceName={chosen.name ?? shortId(chosen.id)}
                liveLines={live.lines}
                onGrantRefused={() => setGrantStale(true)}
              />
            </section>
          </div>
        )}
      </main>

      {connect && (
        <ConnectSheet
          device={connect.device}
          devices={devices}
          initialSection={connect.section}
          focusTokenId={connect.tokenId ?? null}
          onClose={() => setConnect(null)}
        />
      )}

      {notice && (
        <div className="toast" role="status">
          {notice}
          <button type="button" onClick={() => setNotice(null)}>Dismiss</button>
        </div>
      )}

      {renaming && (
        <Modal title={`Rename ${renaming.name ?? shortId(renaming.id)}`} onClose={() => setRenaming(null)}>
          <label htmlFor="rename">Name</label>
          <input
            id="rename"
            value={draftName}
            onChange={(event) => setDraftName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void applyRename();
            }}
            placeholder="leave empty to use the device's own hostname"
          />
          <div className="modal-actions">
            <button type="button" onClick={() => setRenaming(null)}>Cancel</button>
            <button type="button" className="primary" onClick={() => void applyRename()}>Save</button>
          </div>
        </Modal>
      )}

      {removing && (
        <Modal title={`Remove ${removing.name ?? shortId(removing.id)}?`} onClose={() => setRemoving(null)}>
          <p className="hint">
            Its channel closes now and it cannot reconnect. The device keeps running and can be enrolled
            again with a new code.
          </p>
          <div className="modal-actions">
            <button type="button" onClick={() => setRemoving(null)}>Cancel</button>
            <button type="button" className="danger" onClick={() => void applyRemove()}>Remove</button>
          </div>
        </Modal>
      )}
    </div>
  );
}

/**
 * The operator's token list, asked for beside the devices and the audit.
 *
 * The audit resolves each line's `token_id` through it, so it is fetched with the data the page draws —
 * but it is a convenience, not the load. A console that refuses it (an older build without the route,
 * a session that may not read it, a poll that catches a restart) answers with an empty list: the table
 * keeps the ids, which are the fact, and the page keeps working.
 */
function tokensOrEmpty(): Promise<TokenMeta[]> {
  return api.tokens().catch(() => []);
}

function describe(cause: unknown): string {
  if (cause instanceof HttpError) return `${cause.status} ${cause.code}`;
  return cause instanceof Error ? cause.message : String(cause);
}

/**
 * The console's sentence on the page begins with a capital.
 *
 * `/auth`'s refusal is written lowercase because it is also a JSON field ("that token was not
 * accepted; …"); drawn in the panel it is the console speaking to the operator, so it opens like
 * the page's other sentences. Only the first character moves: the flag name and the number are
 * the server's words (audit-39 F6).
 */
function asSentence(text: string): string {
  return text.length === 0 ? text : text[0].toUpperCase() + text.slice(1);
}

/// Whether this page may reload itself once to re-enter Cloudflare Access.
///
/// The guard is a timestamp rather than a flag: a session that expires an hour later in the same tab
/// still gets its reload, while an edge that keeps answering with a login page cannot put the page in
/// a reload loop.
function reloadOnce(): boolean {
  const key = "summrise.access.reload";
  try {
    const now = Date.now();
    const last = Number(window.sessionStorage.getItem(key) ?? "0");
    if (now - last < 20_000) return false;
    window.sessionStorage.setItem(key, String(now));
    return true;
  } catch {
    return false;
  }
}
