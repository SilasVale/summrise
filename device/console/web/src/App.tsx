import { useCallback, useEffect, useRef, useState } from "react";
import logoUrl from "./logo.svg";
import {
  api,
  HttpError,
  NoAnswer,
  type AuditLine,
  type Device,
  type Session,
  type TokenMeta
} from "./api";
import { copyText } from "./copy";
import { lastHeardMs, shortId } from "./format";
import { retrySentence } from "./logic";
import { recordVitals, type VitalsSample } from "./vitals";
import { AUDIT_WINDOW, AuditTable, type AuditScope, type AuditSince } from "./AuditTable";
import type { AuditFilter } from "./auditFilter";
import { mergeActivityWindow, type WindowCounts } from "./activityMerge";
import { activityCountLine } from "./activitySummary";
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
import { HomePage } from "./HomePage";
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

export function App() {
  const [session, setSession] = useState<Session | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
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
    const lines =
      device === null ? await api.audit(AUDIT_WINDOW) : await api.audit(AUDIT_WINDOW, device);
    if (generation > auditReadApplied.current) {
      auditReadApplied.current = generation;
      setAudit(lines);
      setAuditDevice(device);
    }
  }, []);
  // The same index, streamed: what the AI is doing now, not what it did before the last read.
  // `rereadAudit` is how the hook fills the hole a `lagged` frame says the stream left.
  const live = useLiveActivity(Boolean(session?.authenticated), rereadAudit);
  // Activity follows the stream unless the operator paused it: a paused table is a snapshot of the
  // list at that moment, and the lines that arrived since are counted rather than lost.
  const [activityPaused, setActivityPaused] = useState(false);
  const [activityView, setActivityView] = useState<"stream" | "table">("stream");
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
  const [activitySince, setActivitySince] = useState<AuditSince>("all");
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
      setFrozenDevice(null);
      setPausedAt(null);
    } else {
      setPausedAt(Date.now());
      setFrozenActivity(activityLines);
      setFrozenCapped(activityWindow.capped);
      setFrozenAtLimit(activityWindow.atLimit);
      setFrozenCounts(activityCounts);
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
  const [error, setError] = useState<string | null>(null);
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
  // The scope defaults to the chosen machine and follows it while it stays implicit; the timeline
  // shows the same device the table is applying, so neither view can silently disagree.
  const activityScopeNow: AuditScope = activityScope ?? (selected ? "machine" : "all");
  const activityMachine =
    activityScopeNow === "machine" && selected ? selected : activityFilter.device;
  // THE SCOPE IS THE WINDOW. The rows are narrowed to this machine, and when there is one the read
  // asks the console for that machine's newest lines rather than filtering the fleet's window: with
  // `limit` 3, a machine's six older lines can sit entirely behind another machine's eight newer ones.
  const auditScopeDevice = activityMachine === "" ? null : activityMachine;
  auditWindowDevice.current = auditScopeDevice;
  const deviceName = (id: string) =>
    devices.find((device) => device.id === id)?.name ?? shortId(id);
  const auditReadingName = auditScopeDevice === null ? null : deviceName(auditScopeDevice);
  // The rows in hand belong to a machine the scope no longer names. The views say what is happening
  // in one line instead of drawing another window's rows under the new scope. A fleet window is not
  // another machine's: it contains every machine's newest lines, so narrowing it locally is still
  // an answer from a window that includes the scope.
  const auditReadingOther =
    !activityPaused && auditDevice !== null && auditDevice !== auditScopeDevice;
  // A paused list keeps its own window, so its cap sentence names the machine it was frozen from.
  const shownWindowDevice = activityPaused && frozenActivity ? frozenDevice : auditDevice;
  const shownWindowName = shownWindowDevice === null ? null : deviceName(shownWindowDevice);
  // **THE HEADER COUNTS THE WINDOW IT IS SHOWING.** One pass over the rows in hand, so the sentence
  // and the list cannot disagree; a paused page counts the list it froze. A window being replaced is
  // not described: its rows are replaced by the loading line, and the new window's sentence arrives
  // with them. "Today" is the same local midnight the timeline's day label cuts at. The chip's
  // subset gets the hub's sentence: the client calls and the client failures, with nothing the chip
  // has hidden from the list.
  const activityCount =
    shownActivity.length > 0 && !auditReadingOther
      ? activityCountLine(
          shownActivity,
          activityPaused && pausedAt !== null ? pausedAt : Date.now(),
          activityView === "stream" && activityClientsOnly
        )
      : null;

  /**
   * A scope change is a new window, so it is a new read — through the same reader the five-second
   * poll and the lag catch-up use, with the scope's device. `auditReadDevice` is the read already in
   * hand or in flight; when it already names this scope there is nothing to fetch.
   */
  useEffect(() => {
    if (!session?.authenticated) return;
    if (auditReadDevice === auditScopeDevice) return;
    rereadAudit().catch((cause) => {
      setError(
        "The console could not read " +
          (auditScopeDevice === null
            ? "the fleet's audit window"
            : `the audit window for ${shortId(auditScopeDevice)}`) +
          ": " +
          describe(cause)
      );
    });
  }, [session?.authenticated, auditScopeDevice, auditReadDevice, rereadAudit]);

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
    // EACH STREAM ANSWERS FOR ITSELF. A failed /audit must not turn the fleet into "0 devices" or
    // Activity into "no calls": the last known values stay, and one banner says which read failed.
    const [list, auditRead, tokenList] = await Promise.allSettled([
      api.devices(),
      rereadAudit(),
      tokensOrEmpty()
    ]);
    if (list.status === "fulfilled") {
      setDevices(list.value);
      // The poll is the only sampler; the stored last timestamp enforces the 30 s cadence. Sampling
      // after a successful fleet fetch is what keeps an offline device from adding a stale point.
      setVitals(recordVitals(list.value, Date.now()));
    }
    // `rereadAudit` owns the audit state; its rejection is only one of the reads the banner names.
    if (tokenList.status === "fulfilled") setTokens(tokenList.value);
    const failed = [list, auditRead, tokenList].filter(
      (result): result is PromiseRejectedResult => result.status === "rejected"
    );
    if (failed.length > 0) {
      setError("The console could not read " + failed.map((result) => describe(result.reason)).join("; "));
    } else {
      setError(null);
    }
  }, [rereadAudit]);

  const probe = useCallback(async () => {
    // /session decides the page; /health only labels the console's build. Waiting for both meant a
    // console whose /health failed could never render, even with a valid session.
    const state = await api.session();
    setSession(state);
    // The page remembers HOW it was let in, past a local sign-out: a proxy identity cannot be ended
    // from this tab, so the signed-out screen must not hand the operator a token form with no way to
    // use it. `via` is the console's own answer, not a guess from the headers.
    if (state.via === "proxy") setProxied(true);
    if (!state.authenticated) return;
    await refreshData();
    try {
      const health = await api.health();
      setConsoleVersion(health.version);
    } catch {
      /* the version label stays "…" — the page is not blocked on a cosmetic fact */
    }
    setError(null);
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
      setError(describe(cause));
    });
  }, [probe]);

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
    setDevices([]);
    setVitals({});
    setAudit([]);
    setTokens([]);
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
      view: page === "device" && tab === "screen" ? screenView : undefined
    });
    if (window.location.hash !== wanted) window.history.replaceState(null, "", wanted);
  }, [page, tab, screenView, openTool, runsFocus, session?.authenticated]);

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
      setError(describe(cause));
    } finally {
      setBusy(false);
    }
  };

  const applyRename = async () => {
    if (!renaming) return;
    setError(null);
    const name = draftName.trim();
    try {
      await api.rename(renaming.id, name === "" ? null : name);
      setRenaming(null);
      setNotice(name === "" ? "Name cleared; the device's own hostname applies." : `Renamed to ${name}.`);
      await load();
    } catch (cause) {
      setError(describe(cause));
    }
  };

  const applyRemove = async () => {
    if (!removing) return;
    setError(null);
    try {
      await api.remove(removing.id);
      setNotice(`${removing.name ?? shortId(removing.id)} was removed; its channel closed.`);
      setRemoving(null);
      if (selected === removing.id) choose(null);
      await load();
    } catch (cause) {
      setError(describe(cause));
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

  const mintCode = async () => {
    setError(null);
    setEnrolling(true);
    try {
      setCode(await api.enroll());
    } catch (cause) {
      setError(describe(cause));
    } finally {
      setEnrolling(false);
    }
  };

  /// Add a machine is one verb: Home owns the fleet, the code panel is part of it.
  const addMachine = () => {
    setPage("home");
    void mintCode();
  };

  if (session === null) {
    return (
      <div className="login-wrap">
        <section className="panel login">
          <img src={logoUrl} alt="" width={40} height={40} />
          <h1>Connecting to the console</h1>
          {error ? (
            <>
              <p className="error">{error}</p>
              <button type="button" className="primary" onClick={() => void probe().catch((cause) => setError(describe(cause)))}>
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
          {error && <p className="error">{error}</p>}
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
      {error && (
        // POST-SIGN-IN FAILURES HAD NO SURFACE: every handler wrote `error`, and only the signed-out
        // card ever drew it. A rejected enrollment or rename then looked like a click that did
        // nothing. This banner sits above the shell, says the sentence, and stays until dismissed or
        // replaced by the next success.
        <div className="alert-banner" role="alert">
          <span>{error}</span>
          <button type="button" onClick={() => setError(null)}>Dismiss</button>
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
                {activityCount !== null && (
                  <p className="page-sub activity-today">{activityCount}</p>
                )}
                <p className="page-sub">
                  {live.lagged > 0
                    ? `${live.lagged} lines were dropped while this tab was behind; rereading`
                    : activityPaused
                      ? "Paused — the stream is still running; Resume shows what arrived."
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
                  loadingMachine={auditReadingName}
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
                      deviceId !== "" && deviceId === selected ? "machine" : "all"
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
                  lines={shownActivity}
                  capped={shownCapped}
                  atLimit={shownAtLimit}
                  cappedMachine={shownWindowName}
                  windowCounts={shownTableCounts}
                  loading={auditReadingOther}
                  loadingMachine={auditReadingName}
                  devices={devices}
                  tokens={tokens}
                  selectedDeviceId={selected}
                  onOpenRun={openRun}
                  onManageToken={(tokenId) =>
                    setConnect({ section: "tokens", device: chosen, tokenId })
                  }
                  filter={activityFilter}
                  onFilter={setActivityFilter}
                  since={activitySince}
                  onSince={setActivitySince}
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
