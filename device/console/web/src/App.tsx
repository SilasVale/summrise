import { useCallback, useEffect, useRef, useState } from "react";
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
import type { AuditFilter } from "./auditFilter";
import { mergeActivityWindow, type WindowCounts } from "./activityMerge";
import { activityDaySentence } from "./activitySummary";
import { ActivityTimeline } from "./ActivityTimeline";
import { DEVICE_TAB_LABELS, PAGE_LABELS, Rail, type DeviceTab, type Page } from "./Rail";
import { Modal } from "./Modal";
import { ConnectSheet, type ConnectSection } from "./ConnectSheet";
import { TerminalWorkspace } from "./TerminalWorkspace";
import type { Grant } from "./terminal";
import { FilesPage } from "./FilesPage";
import { RunsPage } from "./RunsPage";
import { parseRunLink, runLink } from "./run-link";
import { parseSurfaceHash, surfaceHash } from "./surface-link";
import { ScreenPage } from "./ScreenPage";
import { SystemPage } from "./SystemPage";
import { HomePage, type FleetRead } from "./HomePage";
import { useLiveActivity } from "./useLiveActivity";
import { ToolsTab } from "./ToolsTab";
import { DevicePageShell } from "./DevicePageShell";
import { AccountMenu } from "./AccountMenu";
import { MachineMenu } from "./MachineMenu";
import { useMediaQuery } from "./useMediaQuery";

/// The selected machine survives a reload; it is deliberately not in the URL. A deep link to a
/// device-specific surface without one lands on the picker instead.
const SELECTED_KEY = "summrise.selected.device";

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
 * `key` is the read, not the sentence: the five-second poll fails many times during one outage, and
 * a repeat is the same fact. Replacing the entry by key is the dedupe (audit-38 F2 measured the old
 * banner reading `The console could not read Failed to fetch; Failed to fetch`).
 *
 * `detail` is evidence, never the sentence: the browser's own `Failed to fetch`, or the console's
 * status and code. It is the entry's title, where a curious operator can read it and a sentence
 * cannot present it as the console's words.
 */
type BannerFailure = {
  key: string;
  sentence: string;
  detail?: string;
  retry?: { label: string; run: () => void };
};

/** A failed read's entry: the subject is the sentence, the cause is the title, the retry is offered. */
function readFailure(
  key: string,
  subject: string,
  cause: unknown,
  run: () => void
): BannerFailure {
  return {
    key,
    sentence: `The console could not read ${subject}.`,
    detail: describe(cause),
    retry: { label: `Retry reading ${subject}`, run }
  };
}

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
        api.auditDay(device ?? undefined),
        // The source line is a companion fact, not the window: a failed read here must not fail the
        // rows, and the page then says nothing about the file rather than guessing.
        api.auditSource().catch(() => null)
      ]);
      if (generation > auditReadApplied.current) {
        auditReadApplied.current = generation;
        auditAnsweredDevice.current = device;
        setAudit(lines);
        setAuditDevice(device);
        setDaySummary({ device, day });
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
  // The "Clients only" chip belongs to the page, not only to the timeline: the header must state
  // the same subset the rows show, and the choice survives a switch to the table and back.
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
  // A run id handed from Activity to Runs; RunsPage consumes it once and clears it.
  const [runsFocus, setRunsFocus] = useState<string | null>(initialRun?.runId ?? null);
  /**
   * The shell's named failures, one entry per read and replaced by that read's next failure.
   *
   * THE OLD STATE WAS ONE STRING OF RAW CAUSES: `The console could not read Failed to fetch`, and
   * two failed reads joined with `; ` (audit-38 F2). A failure now carries its subject, the
   * browser's text only as the entry's title, and the retry that reads again.
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
      ? activityDaySentence(
          shownDay.day,
          activityView === "stream" && activityClientsOnly
        )
      : null;
  /**
   * Whether a successful window for the current scope is in hand. The day answer and the rows
   * arrive together, so their scope is the same fact; a failed reread leaves both where they were.
   * This is what tells "no window at all" from "the last window, plus a failed reread".
   */
  const auditHaveWindow =
    daySummary !== null &&
    daySummary.device === auditScopeDevice &&
    auditDevice === auditScopeDevice;
  /** The audit the page is supposed to be drawing, as a sentence's subject. */
  const auditSubject =
    auditScopeDevice === null
      ? "the fleet's audit"
      : `${auditReadingName ?? "the machine"}'s audit`;
  /**
   * The page's own notice, in the hub's voice: the reason the read failed, what the timeline is
   * therefore showing, and the control that reads again. A failed read with no window is the
   * measured F6 state; one over a window already in hand keeps that window and says so.
   */
  const auditFailedNotice =
    auditRead.state === "failed"
      ? auditHaveWindow
        ? `The console could not read ${auditSubject}: ${auditRead.reason}. The rows below are the last read; calls that arrive live are added to them.`
        : `The console could not read ${auditSubject}: ${auditRead.reason}. This timeline shows only calls that arrive live.`
      : null;
  /** No window is in hand: the page's subtitle says so, and the list may not claim emptiness. */
  const auditUnread = auditRead.state !== "ready" && !auditHaveWindow;
  // A scope change already has the timeline's own "Reading <machine>'s window…" line; the page's
  // first read is the one that has no other loading surface.
  const auditReadingFirst =
    auditRead.state === "reading" && auditUnread && !auditReadingOther;

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
    // read keeps its own named banner with the same reader as its retry. Nor may a SLOW /audit hold
    // /fleet back: audit-38 F6 measured known machines drawn as "unknown machine · <hex>" and the
    // selected header missing because the page waited for every read before drawing any of them.
    const retry = () => void refreshData();
    const fleetAnswer = api.devices().then(
      (list) => {
        setDevices(list);
        setFleetRead({ state: "ready" });
        // The poll is the only sampler; the stored last timestamp enforces the 30 s cadence.
        // Sampling after a successful fleet fetch is what keeps an offline device from adding a
        // stale point.
        setVitals(recordVitals(list, Date.now()));
        clearFailure("fleet");
      },
      (reason) => {
        // The last list stays on screen: it is still the console's knowledge. The read state is what
        // keeps it from being drawn as an empty fleet, and the reason is what the page can say.
        setFleetRead({ state: "failed", reason: describe(reason) });
        // The page's own notice carries the reason; the shell's banner names the read, offers the
        // retry, and keeps the browser's text in the title rather than in the sentence.
        reportFailure(readFailure("fleet", "the machine list", reason, retry));
      }
    );
    const tokensAnswer = tokensOrEmpty().then((list) => setTokens(list));
    // `rereadAudit` owns the audit read state; its rejection is only one of the reads the banner
    // names.
    const auditAnswer = rereadAudit().then(
      () => clearFailure("audit"),
      (cause) => reportFailure(readFailure("audit", "the audit", cause, retry))
    );
    // The page's callers still await every read, so a boot or a poll settles only when all three
    // have answered; what changed is WHEN each answer reaches the page.
    await Promise.all([fleetAnswer, tokensAnswer, auditAnswer]);
  }, [rereadAudit, reportFailure, clearFailure]);

  /**
   * A scope change is a new window, so it is a new read — through the same reader the five-second
   * poll and the lag catch-up use, with the scope's device. `auditReadDevice` is the read already in
   * hand or in flight; when it already names this scope there is nothing to fetch.
   *
   * It is declared after `refreshData` because the banner's Retry is that reader: a retry answers
   * both reads and clears each banner its answer settles.
   */
  useEffect(() => {
    if (!session?.authenticated) return;
    if (auditReadDevice === auditScopeDevice) return;
    rereadAudit().catch((cause) => {
      reportFailure(readFailure("audit", "the audit", cause, () => void refreshData()));
    });
  }, [
    session?.authenticated,
    auditScopeDevice,
    auditReadDevice,
    rereadAudit,
    refreshData,
    reportFailure
  ]);

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
    if (window.location.hash) {
      window.history.replaceState(null, "", window.location.pathname + window.location.search);
    }
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
    const run = parseRunLink(window.location.hash);
    if (run && page === "device" && tab === "runs" && runsFocus) return;
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
  }, [page, tab, screenView, openTool, runsFocus, session?.authenticated, activityRange]);

  // **THE ADDRESS BAR IS PART OF THE SURFACE, NOT ONLY A RECORD OF IT.** Every navigation wrote the
  // fragment and nothing read it back: Back and Forward moved the URL and left the view where it was,
  // and a link pasted into a running tab did nothing until a reload. The same parsers the first render
  // uses apply every fragment change, so a hash the app cannot read leaves the screen alone.
  useEffect(() => {
    const apply = () => {
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
    // The fleet's one h1 is its noun: Machines. Home is the hub state, once a machine is chosen.
    const current =
      page === "device"
        ? DEVICE_TAB_LABELS[tab]
        : page === "home" && chosen === null
          ? "Machines"
          : PAGE_LABELS[page];
    document.title = session?.authenticated
      ? `${current} · Summrise console`
      : "Sign in · Summrise console";
  }, [page, tab, chosen, session?.authenticated]);

  const signIn = async () => {
    setBusy(true);
    try {
      // The console answers with Set-Cookie; the page keeps nothing. `load` re-probes `/session`,
      // which now carries the cookie, so the fleet appears without a token in this tab.
      await api.signIn({ token: tokenInput.trim() });
      setTokenInput("");
      await load();
    } catch (cause) {
      reportFailure({
        key: "sign-in",
        sentence: "The console could not sign you in.",
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

  if (!session.authenticated) {
    return (
      <div className="login-wrap">
        <section className="panel login">
          <img src={logoUrl} alt="" width={40} height={40} />
          {proxied ? (
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
                  onClick={() => window.location.reload()}
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
                The operator token is the 64-character secret in the file the console's
                <code> --operator-token-file </code> names, on the console host.
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
          {failures.length > 0 && (
            <p
              className="error"
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
      {failures.length > 0 && (
        // POST-SIGN-IN FAILURES HAD NO SURFACE: every handler wrote an error string, and only the
        // signed-out card ever drew it. The banner keeps each failure its own, names the read that
        // failed, offers that read again, and keeps the technical cause in the title (audit-38 F2).
        // A repeat of the same read replaces its entry instead of stacking another banner.
        <div className="alert-stack">
          {failures.map((failure) => (
            <div className="alert-banner" role="alert" key={failure.key}>
              <span title={failure.detail}>{failure.sentence}</span>
              {failure.retry && (
                <button
                  type="button"
                  aria-label={failure.retry.label}
                  onClick={failure.retry.run}
                >
                  Retry
                </button>
              )}
              <button type="button" onClick={() => clearFailure(failure.key)}>
                Dismiss
              </button>
            </div>
          ))}
        </div>
      )}

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

      <Rail page={page} onPage={go} footer={phone ? undefined : account} />

      <main
        id="main"
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
            onOpen={choose}
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
            onTools={() => {
              setPage("device");
              setTab("tools");
            }}
            onActivity={() => setPage("activity")}
            liveLines={live.lines}
            liveRunning={live.running}
            liveState={live.state}
            tick={pollTick}
            liveRecoveries={live.recoveries}
            onWatchTerminal={(deviceId) => {
              choose(deviceId);
              setPage("device");
              setTab("terminal");
            }}
          />
        )}

        {page === "activity" && (
          <div className="page">
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
                {activityCount !== null && activityRange === null && (
                  <p className="page-sub activity-today">{activityCount}</p>
                )}
                <p className="page-sub">
                  {live.lagged > 0
                    ? `${live.lagged} lines were dropped while this tab was behind; rereading`
                    : activityPaused
                      ? "Paused — the stream is still running; Resume shows what arrived."
                      : auditRead.state === "failed"
                        ? "The audit could not be read."
                        : auditReadingFirst
                          ? "Reading the audit…"
                          : live.state === "live"
                            ? "Following the console's audit as it is written."
                            : live.state === "offline"
                              ? "The live stream is down; this is the last read."
                              : "Opening the live stream."}
                </p>
              </div>
              <div className="page-actions">
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
              // THE HUB'S SENTENCE, ON THE PAGE THAT OWNS THE FILTER. A live-only timeline under a
              // record the console could not read must say which half it is showing, with the reason
              // and the read again — audit-38 F6 measured the page silent about it.
              <div className="notice" role="status">
                <span>{auditFailedNotice}</span>
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
            <section className="panel">
              <div className="stream-bar">
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
                {activityView === "table" && (
                  <p className="hint stream-note">The audit as rows, with filters and CSV export.</p>
                )}
              </div>
              {activityView === "stream" ? (
                <ActivityTimeline
                  lines={shownActivity}
                  devices={devices}
                  tokens={tokens}
                  capped={shownCapped}
                  atLimit={shownAtLimit}
                  cappedMachine={shownWindowName}
                  windowCounts={shownCounts}
                  loading={auditReadingOther}
                  loadingFailed={auditScopeReadFailed}
                  loadingMachine={auditReadingName}
                  unread={auditUnread}
                  source={auditSource}
                  query={activityFilter.tool}
                  onQuery={(tool) => setActivityFilter((current) => ({ ...current, tool }))}
                  clientsOnly={activityClientsOnly}
                  onClientsOnly={setActivityClientsOnly}
                  device={activityMachine}
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
                {tab === "screen" && grant ? (
                  <section className="panel">
                    <ScreenPage
                      grant={grant}
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
                {tab === "machine" && grant ? (
                  <section className="tab-body">
                    <SystemPage
                      grant={grant}
                      device={chosen}
                      deviceOnline={chosen?.online}
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
