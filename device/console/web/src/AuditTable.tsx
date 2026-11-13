import {
  capSentence,
  windowLoadingSentence,
  type WindowCounts
} from "./activityMerge";
import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { api, HttpError, type AuditLine, type AuditSource, type Device, type TokenMeta } from "./api";
import { auditFailed, filterAudit, type AuditFilter } from "./auditFilter";
import {
  dateTimeInputValue,
  parseDateTimeInput,
  presetRange,
  rangeSentence,
  rangeText,
  RANGE_PRESETS,
  type AuditPreset,
  type AuditRange,
  type RangeFacts
} from "./auditTime";
import { auditCsv, auditCsvName, auditJsonl, auditJsonlName } from "./audit-export";
import { downloadFile } from "./download";
import { Moment } from "./Clock";
import { age, shortDeviceName, shortId } from "./format";
import { auditWindowSpan } from "./auditSource";
import { AuditSourceLine } from "./AuditSourceLine";
import { ConsoleCaller } from "./CallerMark";
import { JoinLine, JoinMark, joinRecordSentence } from "./JoinMark";
import { markOf, type JoinMarks } from "./joinMarks";

/**
 * The counts of a window nothing was asked about.
 *
 * A fleet window, a range's answer and a console without a join read all draw this: it asks about
 * nothing, so it says nothing. Naming it once is what keeps `JoinLine` from being called with a
 * fresh object per render.
 */
const EMPTY_JOIN = { one: 0, console: 0, unknown: 0, asked: 0 };
import { tokenFact, tokenLabel } from "./tokenFact";
import { useMediaQuery } from "./useMediaQuery";

/**
 * How many lines the page asks the console for. `/audit?limit=` accepts up to 1000 (`MAX_LOG_LIMIT`
 * in `device/console/src/http.rs`); the page asks for the cap so a full day is on screen and the
 * timeline's "Today" is the day, not the newest slice. P2-12 measured what 1000 rows cost.
 */
export const AUDIT_WINDOW = 1000;

const EMPTY: AuditFilter = { device: "", token: "", tool: "", status: "all" };

/** The Time control's mode: the newest window, one preset, or the custom pair. */
type TimeMode = "window" | AuditPreset | "custom";

/** The table's scope: the machine in the header, or the whole fleet. */
export type AuditScope = "machine" | "all";

/** "a", "a and b", "a, b and c" — the one spelling the filter sentence uses. */
function joinAnd(parts: string[]): string {
  if (parts.length <= 1) return parts[0] ?? "";
  return parts.slice(0, -1).join(", ") + " and " + parts[parts.length - 1];
}

/**
 * The sentence for a filter that hides every line: names what is narrowing the table, so the clear
 * next to it has an object. The caller's `emptyFiltered` replaces it whole.
 */
function filterSentence(args: {
  filter: AuditFilter;
  range: AuditRange | null;
  scope: AuditScope;
  selectedDeviceId: string | null;
  deviceNames: ReadonlyMap<string, string>;
  tokenNames: ReadonlyMap<string, string>;
}): string {
  const { filter, range, scope, selectedDeviceId, deviceNames, tokenNames } = args;
  const parts: string[] = [];
  if (scope === "machine" && selectedDeviceId !== null) {
    parts.push(`the machine ${deviceNames.get(selectedDeviceId) ?? shortId(selectedDeviceId)}`);
  } else if (filter.device !== "") {
    parts.push(`the machine ${deviceNames.get(filter.device) ?? shortId(filter.device)}`);
  }
  if (filter.token !== "") parts.push(`the token ${tokenLabel(filter.token, tokenNames)}`);
  if (filter.tool.trim() !== "") parts.push(`the search “${filter.tool.trim()}”`);
  if (filter.status !== "all") {
    parts.push(filter.status === "ok" ? "succeeded calls" : "failed calls");
  }
  if (range !== null) parts.push(`the range ${rangeText(range)}`);
  if (parts.length === 1 && range !== null) return `No calls in ${parts[0]}.`;
  return `No calls match ${joinAnd(parts)}.`;
}

/**
 * One key per line, stable across filtering and polling.
 *
 * The key is the line's content plus its ordinal among identical lines in the FULL list, not its
 * index in the filtered one: a filter that keeps an open row must keep it open, and the five-second
 * poll returns new objects for the same lines. A content-only key would collapse two identical lines
 * into one; the ordinal is what keeps them apart. The token id is part of the content: two lines that
 * differ only in which token made the call are different facts, so a reordered poll must not move the
 * open row from one token to the other.
 */
function lineKeys(lines: AuditLine[]): Map<AuditLine, string> {
  const seen = new Map<string, number>();
  const keys = new Map<AuditLine, string>();
  for (const line of lines) {
    const base = [
      line.ts_ms,
      line.kind,
      line.device_id ?? "",
      line.token_id ?? "",
      line.tool ?? "",
      line.detail ?? "",
      line.status
    ].join("|");
    const ordinal = seen.get(base) ?? 0;
    seen.set(base, ordinal + 1);
    keys.set(line, `${base}|${ordinal}`);
  }
  return keys;
}

/// The run id `run_detail` wrote into the audit line: `run <id> exited 0 in 42 ms`.
function runIdFromDetail(detail: string | null | undefined): string | null {
  const match = /^run (\S+) /.exec(detail ?? "");
  return match ? match[1] : null;
}

/// The local day a line belongs to; the table groups by this, newest day first.
function dayKey(ms: number): string {
  const at = new Date(ms);
  return `${at.getFullYear()}-${at.getMonth()}-${at.getDate()}`;
}

function dayLabel(ms: number): string {
  const at = new Date(ms);
  const now = new Date();
  const sameDay = (left: Date, right: Date) =>
    left.getFullYear() === right.getFullYear() &&
    left.getMonth() === right.getMonth() &&
    left.getDate() === right.getDate();
  if (sameDay(at, now)) return "Today";
  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
  if (sameDay(at, yesterday)) return "Yesterday";
  return at.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

type Day = { key: string; label: string; lines: AuditLine[] };

/**
 * **THE TABLE OPENS ON THE SAME AMOUNT OF RECORD AS THE TIMELINE (the thirty-first list).** Measured
 * at 1440 on `0.3.273-ux128`: switching to `Table` built **17,074 elements** and a **41,467 px**
 * page in 478 ms — 1,001 rows at once, with no cap sentence and no door — while the Timeline beside
 * it drew **60** and offered `Show the rest of today (865 more)`. The two views are the same data;
 * they must open on the same amount of it. The number is imported from the Timeline rather than
 * written twice, so the two cannot drift.
 */
const TABLE_DAY_VISIBLE = 60;

function byDay(lines: AuditLine[]): Day[] {
  const days: Day[] = [];
  for (const line of lines) {
    const key = dayKey(line.ts_ms);
    const current = days[days.length - 1];
    if (current && current.key === key) current.lines.push(line);
    else days.push({ key, label: dayLabel(line.ts_ms), lines: [line] });
  }
  return days;
}

/**
 * The record: what was called, by which token, with what result.
 *
 * The row is the target — one chevron, no per-row Details button — and the record opens in place.
 * There is no nested scroller: an open record belongs to the page, and the page scrolls. The device
 * column shows the machine's name (the id stays in its title), the exact time is in the title of the
 * relative one, and the record is a two-column grid rather than a fact list stretched across 900 px.
 */
export function AuditTable({
  lines,
  devices,
  tokens,
  selectedDeviceId = null,
  onOpenRun,
  onManageToken,
  empty = "No calls yet. Calls routed through this console are written here; a call made " +
    "straight to a machine's own endpoint is not. Connect a client sets up the credential that " +
    "routes one here.",
  emptyFiltered,
  filter: sharedFilter,
  onFilter,
  range: sharedRange,
  onRange,
  rangeFacts = null,
  rangeError = false,
  scope: sharedScope,
  onScope,
  capped = false,
  cappedMachine = null,
  atLimit = false,
  windowCounts = null,
  loading = false,
  loadingFailed = false,
  loadingMachine = null,
  unread = false,
  source = null,
  join = null
}: {
  lines: AuditLine[];
  capped?: boolean;
  /** The machine whose window was cut, or `null` for the fleet's; the notice names it. */
  cappedMachine?: string | null;
  /**
   * True when the fetched window came back with exactly the requested limit. The page dropped
   * nothing, so this is not `capped`; the sentence names the window's limit and claims nothing
   * about the lines behind it, which the server cannot prove exist.
   */
  atLimit?: boolean;
  /**
   * What this table withheld from the fetched window. Unlike the timeline, the table is the audit
   * and draws the channel boundaries too, so the page hands it `boundaries: 0` and the merged list's
   * length as the drawn count; only repeated keys are withheld here.
   */
  windowCounts?: WindowCounts | null;
  /** True while a changed scope's window is being read; the table says so instead of the old rows. */
  loading?: boolean;
  /** True when that read failed: hide the old rows, but do not keep saying the table is reading. */
  loadingFailed?: boolean;
  /** The machine whose window is being read, or `null` for the fleet's. */
  loadingMachine?: string | null;
  /**
   * True when the page holds no audit window because its read is still out, or failed. The table's
   * empty sentence is a claim about the record, and an unread table cannot make it; the page states
   * the read instead. Rows in hand (live, or a range's answer) are still drawn.
   */
  unread?: boolean;
  /**
   * The file the window is read from, as `/audit/source` served it. The line sits beside the cap
   * sentence; absent or unreadable facts draw nothing rather than a guess.
   */
  source?: AuditSource | null;
  /**
   * **THE MACHINE'S HALF OF EACH CALL (the ledger's third item).** The audit line and the machine's
   * journal both carry one `trace.call`; this is what the machine answered when the page asked which
   * of those traces it still holds. Absent for a fleet window (no one journal answers for it), and
   * then every row is `unknown` and nothing is marked.
   */
  join?: JoinMarks | null;
  devices: Device[];
  tokens: TokenMeta[];
  /** The machine the operator is on; the default scope follows it when it exists. */
  selectedDeviceId?: string | null;
  /** Open a run's record on the device; only the Activity page can navigate to Runs. */
  onOpenRun?: (deviceId: string, runId: string) => void;
  /**
   * Open the Connect sheet on the token a record names, so revoking the credential behind a call is
   * one action from the row rather than a trip through the account menu.
   */
  onManageToken?: (tokenId: string) => void;
  /** The sentence for no lines at all. */
  empty?: string;
  /**
   * The sentence for a filter that matches nothing. The default names the active filters, so a
   * caller that supplies one takes over that explanation as well.
   */
  emptyFiltered?: string;
  /**
   * The six filters are the page's when it wants them shared with the timeline (its search is the
   * table's tool field, its chip the table's device). Omitted, the table owns the same state, which
   * is how it is tested and how it stood alone.
   */
  filter?: AuditFilter;
  onFilter?: (filter: AuditFilter) => void;
  /**
   * The Table's chosen time range, in `[from, to)`. Omitted or `null` is the newest window the
   * page already holds; a range is a different read, so the page hands the table that answer.
   */
  range?: AuditRange | null;
  onRange?: (range: AuditRange | null) => void;
  /** What the range's answer proved about its bounds: where coverage starts, where the scan cut. */
  rangeFacts?: RangeFacts | null;
  /** The range's read failed; the table says so instead of drawing another window's rows. */
  rangeError?: boolean;
  scope?: AuditScope;
  onScope?: (scope: AuditScope) => void;
}) {
  const [ownFilter, setOwnFilter] = useState<AuditFilter>(EMPTY);
  const filter = sharedFilter ?? ownFilter;
  const setFilter = onFilter ?? setOwnFilter;
  // The range is the page's when the page can issue the request; standalone, the table owns it.
  const [ownRange, setOwnRange] = useState<AuditRange | null>(null);
  // `undefined` is "the caller did not take the control", `null` is "the newest window": a parent
  // that passes a range without a callback still owns what the table draws.
  const range = sharedRange !== undefined ? sharedRange : ownRange;
  const setRange = onRange ?? setOwnRange;
  // The control's mode. A link carries a range's moments but not the preset that chose them, so a
  // restored range opens as Custom with its two fields filled; choosing a preset sets its own mode.
  const [timeMode, setTimeMode] = useState<TimeMode>(sharedRange ? "custom" : "window");
  const [customFrom, setCustomFrom] = useState(() =>
    dateTimeInputValue((sharedRange ?? presetRange("hour")).from)
  );
  const [customTo, setCustomTo] = useState(() =>
    dateTimeInputValue((sharedRange ?? presetRange("hour")).to)
  );
  const rangeKey = range === null ? "" : `${range.from}:${range.to}`;
  const lastRangeKey = useRef(rangeKey);
  useEffect(() => {
    if (rangeKey === lastRangeKey.current) return;
    lastRangeKey.current = rangeKey;
    // A refusal answered for the range the operator left; the page no longer draws those rows.
    setExportNotice(null);
    if (range === null) {
      setTimeMode("window");
      return;
    }
    setTimeMode("custom");
    setCustomFrom(dateTimeInputValue(range.from));
    setCustomTo(dateTimeInputValue(range.to));
  }, [rangeKey]);
  const [ownScope, setOwnScope] = useState<AuditScope>(selectedDeviceId ? "machine" : "all");
  const scope = sharedScope ?? ownScope;
  const setScope = onScope ?? setOwnScope;
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  // **WHICH DAYS THE OPERATOR HAS OPENED PAST THE FOLD.** Same idea as the Timeline's `openDays`: a
  // day opens on its newest rows and one door states the rest; pressing it draws the whole day.
  const [openDays, setOpenDays] = useState<Record<string, boolean>>({});
  // THE SCOPE IS THE SELECTION. Activity follows the machine in the header unless the operator asks
  // for the fleet's record.
  const applied = useMemo(
    () =>
      scope === "machine" && selectedDeviceId
        ? { ...filter, device: selectedDeviceId }
        : filter,
    [scope, selectedDeviceId, filter]
  );
  const shown = useMemo(() => filterAudit(lines, applied), [lines, applied]);
  const keys = useMemo(() => lineKeys(lines), [lines]);
  // THE WINDOW'S SPAN, NOT THE FILE'S LIFETIME (audit-44 F10). `lines` is the window the page holds
  // — the fetched list plus live lines — before this table's filters narrow the rows. A chosen range
  // is a different read with its own sentence, so it draws the file's facts without this window's.
  const windowSpan = useMemo(() => auditWindowSpan(lines), [lines]);
  const tokenNames = useMemo(() => new Map(tokens.map((entry) => [entry.id, entry.name])), [tokens]);
  const days = useMemo(() => byDay(shown), [shown]);

  // Only the tokens that appear in the window the page holds: the control stays short, and an option
  // always matches at least one line. Sorted by the label the operator reads rather than by id, so a
  // poll that reorders the lines cannot reorder the options under their caret.
  const tokenOptions = useMemo(() => {
    const ids = new Set(
      lines
        .map((line) => line.token_id)
        .filter((id): id is string => id != null && id !== "")
    );
    return Array.from(ids)
      .map((id) => ({ id, label: tokenLabel(id, tokenNames) }))
      .sort((left, right) => left.label.localeCompare(right.label) || left.id.localeCompare(right.id));
  }, [lines, tokenNames]);

  const fleet = new Set(devices.map((device) => device.id));
  const deviceNames = useMemo(
    () => new Map(devices.map((device) => [device.id, device.name ?? shortId(device.id)])),
    [devices]
  );
  // The fleet's names are what say which prefix they share; a retired id is not in it and keeps
  // its whole short id.
  const fleetNames = useMemo(() => Array.from(deviceNames.values()), [deviceNames]);
  const [copied, setCopied] = useState<string | null>(null);
  // The export route's refusal (a range the scan bound cannot prove) is a sentence, not a file.
  // It remembers the range it answered for: a refusal arriving after the operator chose another
  // range belongs to rows the page no longer shows.
  const [exportNotice, setExportNotice] = useState<{ text: string; rangeKey: string } | null>(null);
  // THE OPENER IS IN THE ROW, NOT AT ITS FAR END. A phone cannot see a sixth column, and the old
  // 44 rem table made opening one record cost a sideways swipe; below 640 px the disclosure is the
  // When cell and the chevron column is not rendered at all. The desktop table is unchanged.
  const wide = useMediaQuery("(min-width: 640px)");
  // The phone folds the filters behind one disclosure (audit-41 F4): the measured form spent 296 px
  // of the first screen on eight controls and put the first record at y=746, under the rail. `narrow`
  // is its own query, not `!wide`: an environment without `matchMedia` answers false here and keeps
  // the desktop placement, exactly as `useMediaQuery` documents.
  const narrow = useMediaQuery("(max-width: 639px)");
  // The one Export control is a menu; a menu is a layer, not a section, so a press elsewhere closes it.
  const exportRef = useRef<HTMLDetailsElement>(null);
  useEffect(() => {
    const onDown = (event: MouseEvent) => {
      if (exportRef.current && !exportRef.current.contains(event.target as Node)) {
        exportRef.current.removeAttribute("open");
      }
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, []);
  const retired = Array.from(
    new Set(
      lines
        .map((line) => line.device_id)
        .filter((id): id is string => id != null && id !== "" && !fleet.has(id))
    )
  ).sort();

  // How many of the table's own filters are narrowing it: the disclosure at 390 says the count,
  // because a filter the operator cannot see must not be a filter they forget. The scope is not in
  // it (on a machine page it is the default, not a choice) and neither is a hidden Device control.
  const filterCount =
    (filter.device !== "" ? 1 : 0) +
    (filter.token !== "" ? 1 : 0) +
    (filter.tool.trim() !== "" ? 1 : 0) +
    (filter.status !== "all" ? 1 : 0) +
    (range !== null ? 1 : 0);
  const hasFilter = filterCount > 0;

  const clear = () => {
    setFilter(EMPTY);
    // The range is a filter too: clearing must put the newest window back, which is a prop change
    // the page answers by drawing the window it already holds.
    lastRangeKey.current = "";
    setExportNotice(null);
    setTimeMode("window");
    setRange(null);
    setOpen(new Set());
  };

  // The empty state's clear also widens the machine scope, which the toolbar's Clear leaves alone:
  // an empty state the operator cannot leave is a dead end, and the scope is how Activity follows
  // the header's machine by default.
  const clearAll = () => {
    clear();
    setScope("all");
  };

  /** Apply one range (or the newest window) and remember how the control should name it. */
  const applyRange = (next: AuditRange | null, mode: TimeMode) => {
    lastRangeKey.current = next === null ? "" : `${next.from}:${next.to}`;
    // The control moved: any refusal still on screen answered for the range it just left.
    setExportNotice(null);
    setTimeMode(mode);
    setRange(next);
  };

  /** The custom pair as a range, or `null` while either field is not one. */
  const customRange = (): AuditRange | null => {
    const from = parseDateTimeInput(customFrom);
    const to = parseDateTimeInput(customTo);
    if (from === null || to === null || from >= to) return null;
    return { from, to };
  };

  const chooseTime = (value: string) => {
    if (value === "window") {
      applyRange(null, "window");
      return;
    }
    if (value === "custom") {
      const base = range ?? presetRange("hour");
      setCustomFrom(dateTimeInputValue(base.from));
      setCustomTo(dateTimeInputValue(base.to));
      setTimeMode("custom");
      return;
    }
    const preset = value as AuditPreset;
    applyRange(presetRange(preset), preset);
  };

  /** Put the one Export menu away: it is a layer, not a section of the form. */
  const closeExport = () => exportRef.current?.removeAttribute("open");

  const exportCsv = () => {
    if (shown.length === 0) return;
    downloadFile(
      auditCsvName(new Date()),
      auditCsv(
        shown,
        (id) => (id ? (deviceNames.get(id) ?? shortId(id)) : ""),
        (id) => (id ? tokenLabel(id, tokenNames) : "")
      ),
      "text/csv;charset=utf-8"
    );
  };

  /**
   * Save what the Time control drew. A chosen range is the console's file — its filename, its
   * bounds headers, its refusal when the scan bound cannot prove it. The newest window has no
   * bounds to ask for, so the lines in hand are written here, in the same chronological JSON Lines.
   */
  const exportFile = async () => {
    if (range === null) {
      if (lines.length === 0) return;
      downloadFile(auditJsonlName(lines), auditJsonl(lines), "application/x-ndjson");
      setExportNotice(null);
      return;
    }
    // The refusal names the range it answered for, even if it lands after the operator left it.
    const askedRange = rangeKey;
    try {
      const answer = await api.auditExport(
        range,
        applied.device === "" ? undefined : applied.device
      );
      downloadFile(answer.filename, answer.body, "application/x-ndjson");
      // A later answer that succeeds supersedes the refusal the operator retried.
      setExportNotice(null);
    } catch (error) {
      setExportNotice({
        text:
          error instanceof HttpError
            ? `The console refused this export: ${error.message}`
            : "The console could not answer the export.",
        rangeKey: askedRange
      });
    }
  };

  const copyDevice = async (id: string, key: string) => {
    try {
      await navigator.clipboard.writeText(id);
      setCopied(key);
    } catch {
      // The id is selectable text; a refused clipboard is not an error the table should shout about.
    }
  };

  const toggle = (key: string) => {
    setOpen((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const narrowed =
    (scope === "machine" && selectedDeviceId !== null) ||
    filter.device !== "" ||
    filter.token !== "" ||
    filter.tool.trim() !== "" ||
    filter.status !== "all" ||
    range !== null;
  // **THE SCOPE IS THE REQUEST, NOT A FILTER.** A machine scope is what the window was fetched
  // for, so its count already describes it; these are the controls that narrow the rows in hand.
  const filterActive =
    filter.token !== "" ||
    filter.tool.trim() !== "" ||
    filter.status !== "all" ||
    (filter.device !== "" && (scope !== "machine" || selectedDeviceId === null));
  const match = filterActive ? { matched: shown.length, drawn: lines.length } : null;
  const emptySentence = narrowed
    ? (emptyFiltered ??
      filterSentence({
        filter,
        range,
        scope,
        selectedDeviceId,
        deviceNames,
        tokenNames
      }))
    : empty;

  const disclosure = (key: string, isOpen: boolean, line: AuditLine) => (
    <button
      type="button"
      className="audit-disclosure"
      aria-expanded={isOpen}
      aria-label={`${isOpen ? "Hide" : "Show"} the record for ${line.tool ?? line.kind}`}
      onClick={() => toggle(key)}
    >
      <span className="audit-chevron" aria-hidden="true">
        {isOpen ? "▾" : "›"}
      </span>
    </button>
  );

  return (
    <div>
      <div className="audit-toolbar">
        {/* **ONE ROW, ONE EXPORT (audit-41 F4).** The measured form was eight controls in two rows,
            `Export JSONL` in the first and `Export CSV` in the second, and its own sentence named one
            format. Above the phone the fields share one band and the export is the one control at its
            end; at 390 the fields fold behind a counted `Filters` summary so the first record is on
            the first screen. The count is the filters the table is actually applying, not the
            controls it happens to draw: a hidden Device field is not a filter. */}
        <details className="audit-filter-fold" open={!narrow}>
          <summary className="audit-filter-summary">
            Filters{filterCount > 0 && ` (${filterCount})`}
          </summary>
          <div className="audit-filter-fields">
            {/* **ONE VOCABULARY: THE FILTER NAMES THE COLUMN IT NARROWS (the hundred-and-thirty-sixth
                list).** Measured 2026-11-11 at 1440: the toolbar named its fields TIME and TOOL OR DETAIL
                while the header row of the very table beneath them said When and What happened — two
                words for one column, on one screen. The labels are the headers now, and the scope pair,
                which had only an accessible name, carries a visible one in the same row of labels. */}
            <div className="audit-field">
              <span id="audit-scope-label">Scope</span>
              <div className="seg" role="group" aria-labelledby="audit-scope-label">
                <button
                  type="button"
                  aria-pressed={scope === "machine"}
                  disabled={!selectedDeviceId}
                  onClick={() => setScope("machine")}
                >
                  This machine
                </button>
                <button type="button" aria-pressed={scope === "all"} onClick={() => setScope("all")}>
                  All machines
                </button>
              </div>
            </div>
            <label className="audit-field">
              <span>When</span>
              <select value={timeMode} onChange={(event) => chooseTime(event.target.value)}>
                {/* **THE WINDOW SAYS WHAT IT IS.** "All time" named the newest 1,000 lines as the
                    whole record; the page has always drawn the newest window, and now the words do. */}
                <option value="window">Newest window</option>
                {RANGE_PRESETS.map((preset) => (
                  <option key={preset.value} value={preset.value}>
                    {preset.label}
                  </option>
                ))}
                <option value="custom">Custom…</option>
              </select>
            </label>
            {timeMode === "custom" && (
              <>
                <label className="audit-field">
                  <span>From</span>
                  <input
                    type="datetime-local"
                    value={customFrom}
                    onChange={(event) => setCustomFrom(event.target.value)}
                  />
                </label>
                <label className="audit-field">
                  <span>To</span>
                  <input
                    type="datetime-local"
                    value={customTo}
                    onChange={(event) => setCustomTo(event.target.value)}
                  />
                </label>
                <button
                  type="button"
                  disabled={customRange() === null}
                  onClick={() => {
                    const chosen = customRange();
                    if (chosen !== null) applyRange(chosen, "custom");
                  }}
                >
                  Apply
                </button>
              </>
            )}
            {scope === "all" && (
              <label className="audit-field">
                <span>Device</span>
                <select
                  value={filter.device}
                  onChange={(event) => setFilter({ ...filter, device: event.target.value })}
                >
                  <option value="">All devices</option>
                  {devices.map((device) => (
                    <option key={device.id} value={device.id}>
                      {device.name ?? shortId(device.id)}
                    </option>
                  ))}
                  {retired.map((id) => (
                    <option key={id} value={id}>
                      {shortId(id)}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {tokenOptions.length > 0 && (
              <label className="audit-field">
                <span>Token</span>
                <select
                  value={filter.token}
                  onChange={(event) => setFilter({ ...filter, token: event.target.value })}
                >
                  <option value="">All tokens</option>
                  {tokenOptions.map((option) => (
                    <option key={option.id} value={option.id}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <label className="audit-field tool">
              <span>What happened</span>
              <input
                type="search"
                value={filter.tool}
                placeholder="filter by tool or detail"
                onChange={(event) => setFilter({ ...filter, tool: event.target.value })}
              />
            </label>
            <label className="audit-field">
              <span>Status</span>
              <select
                value={filter.status}
                onChange={(event) =>
                  setFilter({ ...filter, status: event.target.value as AuditFilter["status"] })
                }
              >
                <option value="all">All</option>
                <option value="ok">Succeeded</option>
                <option value="failed">Failed</option>
              </select>
            </label>
            {hasFilter && (
              <button type="button" onClick={clear}>
                Clear
              </button>
            )}
          </div>
        </details>
        {/* **ONE EXPORT CONTROL, BOTH FORMATS.** The measured card put `Export JSONL` beside Time
            and `Export CSV` at the row's far end, with the formats in two places (audit-41 F4). The
            summary is the one control; the menu holds the two files and their own names say what
            each one is: the CSV is the rows on screen, the JSON Lines is the window in hand (or the
            drawn range), oldest first, and the table's other filters do not apply to it. */}
        <details className="audit-export-menu" ref={exportRef}>
          <summary
            className="audit-export-summary"
            title="Download the rows on screen as CSV, or the window in hand (or the drawn range) as JSON Lines"
          >
            Export
          </summary>
          <div className="audit-export-formats">
            {/* **THE RANGE THE OPERATOR CHOSE IS THE RANGE THAT LEAVES.** A chosen time range is the
                console's own file — its filename, its bounds headers, its refusal when the scan
                bound cannot prove it. The newest window has no bounds to ask for, so the lines in
                hand are written here. A range that drew nothing leaves nothing: disabled when the
                range's own answer held no lines, because the route would answer with a zero-byte
                file and no sentence. The count is the range's answer, not the filters' rows. */}
            <button
              type="button"
              className="audit-export"
              disabled={loading || rangeError || lines.length === 0}
              onClick={() => {
                closeExport();
                void exportFile();
              }}
              title={
                range === null
                  ? `Export JSONL: the newest window in hand — ${lines.length} ${
                      lines.length === 1 ? "line" : "lines"
                    }, oldest first; the table's other filters do not apply`
                  : `Export JSONL: the drawn range ${rangeText(range)}, oldest first; the table's other filters do not apply`
              }
              aria-label={
                range === null
                  ? `Export JSONL: the newest window in hand — ${lines.length} ${
                      lines.length === 1 ? "line" : "lines"
                    }, oldest first; the table's other filters do not apply`
                  : `Export JSONL: the drawn range ${rangeText(range)}, oldest first; the table's other filters do not apply`
              }
            >
              Export JSONL
            </button>
            {/* **A MENU WITH ONE ITEM HIDES THE FEATURE.** "Export CSV" was behind an unlabelled
                "⋯" whose only content it was: a discovery problem for the one action here that
                leaves the page with data. It is a labelled item in the one Export menu. */}
            <button
              type="button"
              className="audit-export"
              disabled={shown.length === 0}
              onClick={() => {
                closeExport();
                exportCsv();
              }}
              title="Download the rows on screen as CSV"
            >
              Export CSV
            </button>
          </div>
        </details>
      </div>

      {exportNotice !== null && exportNotice.rangeKey === rangeKey && (
        // **A REFUSAL IS AN ALERT, NOT A HINT.** It says the operator's request failed, so it wears
        // the console's error surface and offers Dismiss, inline above the table it explains.
        <div className="alert-inline" role="alert">
          <span>{exportNotice.text}</span>
          <button type="button" onClick={() => setExportNotice(null)}>
            Dismiss
          </button>
        </div>
      )}

      {/* **A RANGE'S SENTENCE IS THE FETCH'S, NOT THE FILTERS'.** It stays above the empty state
          and the table alike: a range that drew nothing still has to say which range it asked for
          and where the file's coverage starts, and a range that drew 342 lines says so before the
          operator's token/tool filters narrow the rows further. */}
      {range !== null && !loading && (
        <p className="hint audit-cap">
          {rangeError
            ? "The console could not read this range."
            : rangeSentence(range, lines.length, rangeFacts, match)}
        </p>
      )}
      {range !== null && rangeError ? null : loading ? (
        loadingFailed ? null : (
          <p className="hint audit-loading">{windowLoadingSentence(loadingMachine)}</p>
        )
      ) : shown.length === 0 ? (
        unread && range === null ? null : (
          <div className="audit-empty">
            <p className="hint">{emptySentence}</p>
            {narrowed && (
              <button type="button" onClick={clearAll}>
                Clear filters
              </button>
            )}
          </div>
        )
      ) : (
        <div className="table-scroll" tabIndex={0} role="region" aria-label="Audit lines">
          {range === null && (capped || atLimit) && (
            // One sentence, two facts: rows were cut, or the request filled the window. Only the
            // second is true when a duplicate drop left fewer than 1000 rows on screen.
            <p className="hint audit-cap">
              {capSentence(
                AUDIT_WINDOW,
                cappedMachine,
                capped ? "capped" : "at-limit",
                windowCounts ?? null,
                match
              )}
            </p>
          )}
          <AuditSourceLine source={source} window={range === null ? windowSpan : null} />
          {range === null && (
            <JoinLine counts={join?.counts ?? EMPTY_JOIN} error={join?.error ?? null} />
          )}
          <table className="audit-table">
            <thead>
              <tr>
                <th>When</th>
                <th>Device</th>
                <th>Token</th>
                <th>What happened</th>
                <th>Status</th>
                {wide && (
                  <th>
                    <span className="sr-only">Record</span>
                  </th>
                )}
              </tr>
            </thead>
            {days.map((day) => (
              <tbody key={day.key}>
                <tr className="audit-day">
                  <th colSpan={wide ? 6 : 5}>{day.label}</th>
                </tr>
                {(() => {
                  const expanded = openDays[day.key] === true;
                  const shownLines = expanded
                    ? day.lines
                    : day.lines.slice(0, TABLE_DAY_VISIBLE);
                  return shownLines.map((line) => {
                  const key = keys.get(line) ?? String(line.ts_ms);
                  const isOpen = open.has(key);
                  const failed = auditFailed(line);
                  const deviceError = failed && line.status < 400;
                  const runId = line.tool === "runs.exec" ? runIdFromDetail(line.detail) : null;
                  const deviceName = line.device_id
                    ? deviceNames.get(line.device_id) ?? shortId(line.device_id)
                    : "—";
                  const lineJoin = markOf(join, line);
                  return (
                    <Fragment key={key}>
                      <tr className="audit-row">
                        <td className="audit-when" title={new Date(line.ts_ms).toISOString()}>
                          {!wide && disclosure(key, isOpen, line)}
                          {/* **ONE CLOCK, WITH ONE SPACE BEFORE THE AGE (audit-43 F8).** The cell
                              said `08:46:18· 8s ago` — no space, because the gap was a 4 px margin
                              rather than a character. The shared `Moment` draws the clock and the
                              age with the one literal separator, and the time's `title` carries the
                              exact instant the cell never offered. */}
                          <Moment
                            ts={line.ts_ms}
                            age={age(line.ts_ms)}
                            ageClassName="audit-age"
                          />
                        </td>
                        <td title={line.device_id ? `${deviceName} (${line.device_id})` : deviceName}>
                          {wide ? (
                            deviceName
                          ) : (
                            <>
                              {/* THE PHONE COLUMN IS 67 px. The measured cell held
                                  "DESKTOP-9VH92BJ" in three lines and grew the row to 91 px; the
                                  part after the fleet's shared prefix fits on one line. The full
                                  name stays the cell's accessible name, and its title carries the
                                  name and the id a hover cannot show on a phone. */}
                              <span className="sr-only">{deviceName}</span>
                              <span className="audit-device-short" aria-hidden="true">
                                {line.device_id
                                  ? shortDeviceName(deviceName, fleetNames)
                                  : deviceName}
                              </span>
                            </>
                          )}
                          {line.caller === "console" && (
                            <>
                              {" "}
                              <ConsoleCaller className="audit-caller" />
                            </>
                          )}
                        </td>
                        <td className={line.token_id ? "audit-token" : "audit-token is-empty"}>
                          {line.token_id ? (
                            <span title={line.token_id}>{tokenLabel(line.token_id, tokenNames)}</span>
                          ) : (
                            "—"
                          )}
                        </td>
                        <td title={line.tool ?? undefined}>
                          {line.sentence ? (
                            <span>{line.sentence}</span>
                          ) : (
                            <span className="mono">{line.tool ?? line.detail ?? ""}</span>
                          )}
                          {deviceError && (
                            <>
                              {" "}
                              <span className="chip chip-warn" title={line.detail ?? undefined}>
                                error
                              </span>
                            </>
                          )}
                          {/* The row's own join, quiet: only a call whose other half has rotated out
                              of the machine's journal earns a word, and the record explains it. */}
                          {lineJoin === "console" && (
                            <>
                              {" "}
                              <JoinMark kind="console" />
                            </>
                          )}
                        </td>
                        <td>
                          <span
                            className={
                              "chip " +
                              (line.status >= 400 ? "chip-bad" : deviceError ? "chip-warn" : "chip-ok")
                            }
                            title={
                              deviceError
                                ? "HTTP 200: the device answered with a JSON-RPC error, shown in the record"
                                : undefined
                            }
                          >
                            {line.status}
                          </span>
                        </td>
                        {wide && (
                          <td className="audit-chevron-cell">{disclosure(key, isOpen, line)}</td>
                        )}
                      </tr>
                      {isOpen && (
                        <tr className="audit-record-row">
                          <td colSpan={wide ? 6 : 5}>
                            <dl className="audit-record">
                              <dt>Time</dt>
                              <dd title={new Date(line.ts_ms).toISOString()}>
                                {new Date(line.ts_ms).toLocaleString()} · {age(line.ts_ms)}
                              </dd>
                              <dt>Device</dt>
                              <dd>
                                {deviceName}
                                {line.device_id && (
                                  <button
                                    type="button"
                                    className="mini audit-copy"
                                    onClick={() => void copyDevice(line.device_id!, key)}
                                  >
                                    {copied === key ? "Copied" : "Copy id"}
                                  </button>
                                )}
                              </dd>
                              <dt>Token</dt>
                              <dd>
                                {tokenFact(line, tokenNames)}
                                {line.token_id && onManageToken && (
                                  <button
                                    type="button"
                                    className="mini"
                                    onClick={() => onManageToken(line.token_id as string)}
                                  >
                                    Manage
                                  </button>
                                )}
                              </dd>
                              <dt>Tool</dt>
                              <dd className="mono">{line.tool ?? "—"}</dd>
                              <dt>Detail</dt>
                              <dd className="mono">{line.detail ?? "—"}</dd>
                              {runId && line.device_id && onOpenRun && (
                                <>
                                  <dt>Run</dt>
                                  <dd>
                                    <button
                                      type="button"
                                      className="mini"
                                      onClick={() => onOpenRun(line.device_id!, runId)}
                                    >
                                      Open in Runs
                                    </button>
                                  </dd>
                                </>
                              )}
                              <dt>Status</dt>
                              <dd>{line.status}</dd>
                              {/* **THE JOIN, IN THE RECORD'S OWN WORDS.** Only a call both records
                                  hold, or one only this console holds, has anything to say: an
                                  unchecked row (a fleet window, an older machine, a failed read) is
                                  a fact about the page, not about the call, and the row above already
                                  says the page could not check it. */}
                              {joinRecordSentence(lineJoin) !== null && (
                                <>
                                  <dt>Journal</dt>
                                  <dd className="audit-join-fact">{joinRecordSentence(lineJoin)}</dd>
                                </>
                              )}
                            </dl>
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  );
                  });
                })()}
                {openDays[day.key] !== true && day.lines.length > TABLE_DAY_VISIBLE && (
                  // **ONE DOOR, THE DAY'S OWN REST ON IT.** It is drawn outside the row map, after
                  // the rows it reveals, and its label states how many are behind it so a reader
                  // knows what the press costs before making it.
                  <tr className="audit-more-row">
                    <td colSpan={wide ? 6 : 5}>
                      <button
                        type="button"
                        className="audit-more"
                        onClick={() =>
                          setOpenDays((current) => ({ ...current, [day.key]: true }))
                        }
                      >
                        Show the rest of {day.label} ({day.lines.length - TABLE_DAY_VISIBLE} more)
                      </button>
                    </td>
                  </tr>
                )}
              </tbody>
            ))}
          </table>
        </div>
      )}
    </div>
  );
}
