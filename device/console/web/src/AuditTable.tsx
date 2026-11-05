import { capSentence, windowLoadingSentence } from "./activityMerge";
import { Fragment, useMemo, useState } from "react";
import type { AuditLine, Device, TokenMeta } from "./api";
import { auditFailed, filterAudit, type AuditFilter } from "./auditFilter";
import { auditCsv, auditCsvName } from "./audit-export";
import { downloadFile } from "./download";
import { age, shortId } from "./format";
import { ConsoleCaller } from "./CallerMark";

/**
 * How many lines the page asks the console for. `/audit?limit=` accepts up to 1000 (`MAX_LOG_LIMIT`
 * in `device/console/src/http.rs`); the page asks for the cap so a full day is on screen and the
 * timeline's "Today" is the day, not the newest slice. P2-12 measured what 1000 rows cost.
 */
export const AUDIT_WINDOW = 1000;

const EMPTY: AuditFilter = { device: "", token: "", tool: "", status: "all" };

/// The toolbar's time window. The table already holds the newest 1000 lines, so this only narrows
/// what is on screen; it never asks the console for a different slice.
export type AuditSince = "all" | "hour" | "day" | "week";
const SINCE_MS: Record<Exclude<AuditSince, "all">, number> = {
  hour: 60 * 60_000,
  day: 24 * 60 * 60_000,
  week: 7 * 24 * 60 * 60_000
};

/** The table's scope: the machine in the header, or the whole fleet. */
export type AuditScope = "machine" | "all";

/**
 * What to call one token in a cell or an option.
 *
 * The name is the operator's, and it is the thing they recognise; the id is what the console recorded
 * and what the filter compares. So a name the list knows wins, and an id it does not (a revoked token,
 * one created in another tab, or the whole list when `GET /tokens` failed) falls back to its short
 * form, which still tells two tokens apart.
 */
function tokenLabel(id: string, names: ReadonlyMap<string, string>): string {
  const name = names.get(id);
  return name != null && name !== "" ? name : shortId(id);
}

/** How the table's time window reads in a sentence. */
const SINCE_LABEL: Record<Exclude<AuditSince, "all">, string> = {
  hour: "last hour",
  day: "last day",
  week: "last week"
};

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
  since: AuditSince;
  scope: AuditScope;
  selectedDeviceId: string | null;
  deviceNames: ReadonlyMap<string, string>;
  tokenNames: ReadonlyMap<string, string>;
}): string {
  const { filter, since, scope, selectedDeviceId, deviceNames, tokenNames } = args;
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
  if (since !== "all") parts.push(`the ${SINCE_LABEL[since]}`);
  if (parts.length === 1 && since !== "all") return `No calls in ${parts[0]}.`;
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
  empty = "No calls yet. Every call a client makes is written here; when an AI client calls a " +
    "machine with its token, its first line appears. Connect a client sets one up.",
  emptyFiltered,
  filter: sharedFilter,
  onFilter,
  since: sharedSince,
  onSince,
  scope: sharedScope,
  onScope,
  capped = false,
  cappedMachine = null,
  loading = false,
  loadingMachine = null
}: {
  lines: AuditLine[];
  capped?: boolean;
  /** The machine whose window was cut, or `null` for the fleet's; the notice names it. */
  cappedMachine?: string | null;
  /** True while a changed scope's window is being read; the table says so instead of the old rows. */
  loading?: boolean;
  /** The machine whose window is being read, or `null` for the fleet's. */
  loadingMachine?: string | null;
  devices: Device[];
  tokens: TokenMeta[];
  /** The machine the operator is on; the default scope follows it when it exists. */
  selectedDeviceId?: string | null;
  /** Open a run's record on the device; only the Activity page can navigate to Runs. */
  onOpenRun?: (deviceId: string, runId: string) => void;
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
  since?: AuditSince;
  onSince?: (since: AuditSince) => void;
  scope?: AuditScope;
  onScope?: (scope: AuditScope) => void;
}) {
  const [ownFilter, setOwnFilter] = useState<AuditFilter>(EMPTY);
  const filter = sharedFilter ?? ownFilter;
  const setFilter = onFilter ?? setOwnFilter;
  const [ownSince, setOwnSince] = useState<AuditSince>("all");
  const since = sharedSince ?? ownSince;
  const setSince = onSince ?? setOwnSince;
  const [ownScope, setOwnScope] = useState<AuditScope>(selectedDeviceId ? "machine" : "all");
  const scope = sharedScope ?? ownScope;
  const setScope = onScope ?? setOwnScope;
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  // THE SCOPE IS THE SELECTION. Activity follows the machine in the header unless the operator asks
  // for the fleet's record.
  const applied = useMemo(
    () =>
      scope === "machine" && selectedDeviceId
        ? { ...filter, device: selectedDeviceId }
        : filter,
    [scope, selectedDeviceId, filter]
  );
  const windowed = useMemo(() => {
    if (since === "all") return lines;
    const cutoff = Date.now() - SINCE_MS[since];
    return lines.filter((line) => line.ts_ms >= cutoff);
  }, [lines, since]);
  const shown = useMemo(() => filterAudit(windowed, applied), [windowed, applied]);
  const keys = useMemo(() => lineKeys(lines), [lines]);
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
  const [copied, setCopied] = useState<string | null>(null);
  const retired = Array.from(
    new Set(
      lines
        .map((line) => line.device_id)
        .filter((id): id is string => id != null && id !== "" && !fleet.has(id))
    )
  ).sort();

  const hasFilter =
    filter.device !== "" ||
    filter.token !== "" ||
    filter.tool.trim() !== "" ||
    filter.status !== "all" ||
    since !== "all";

  const clear = () => {
    setFilter(EMPTY);
    setSince("all");
    setOpen(new Set());
  };

  // The empty state's clear also widens the machine scope, which the toolbar's Clear leaves alone:
  // an empty state the operator cannot leave is a dead end, and the scope is how Activity follows
  // the header's machine by default.
  const clearAll = () => {
    clear();
    setScope("all");
  };

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
    since !== "all";
  const emptySentence = narrowed
    ? (emptyFiltered ??
      filterSentence({
        filter,
        since,
        scope,
        selectedDeviceId,
        deviceNames,
        tokenNames
      }))
    : empty;

  return (
    <div>
      <div className="audit-toolbar">
        <div className="seg" role="group" aria-label="Audit scope">
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
        <label className="audit-field">
          <span>Time</span>
          <select value={since} onChange={(event) => setSince(event.target.value as AuditSince)}>
            <option value="all">All time</option>
            <option value="hour">Last hour</option>
            <option value="day">Last day</option>
            <option value="week">Last week</option>
          </select>
        </label>
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
          <span>Tool or detail</span>
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
        {/* **A MENU WITH ONE ITEM HIDES THE FEATURE.** "Export CSV" was behind an unlabelled "⋯"
            whose only content it was: two clicks and a discovery problem for the one action here that
            leaves the page with data. It is a button. */}
        <button
          type="button"
          className="audit-export"
          disabled={shown.length === 0}
          onClick={exportCsv}
          title="Download the rows on screen as CSV"
        >
          Export CSV
        </button>
      </div>

      {loading ? (
        <p className="hint audit-loading">{windowLoadingSentence(loadingMachine)}</p>
      ) : shown.length === 0 ? (
        <div className="audit-empty">
          <p className="hint">{emptySentence}</p>
          {narrowed && (
            <button type="button" onClick={clearAll}>
              Clear filters
            </button>
          )}
        </div>
      ) : (
        <div className="table-scroll" tabIndex={0} role="region" aria-label="Audit lines">
          {capped && (
            <p className="hint audit-cap">{capSentence(AUDIT_WINDOW, cappedMachine)}</p>
          )}
          <table className="audit-table">
            <thead>
              <tr>
                <th>When</th>
                <th>Device</th>
                <th>Token</th>
                <th>What happened</th>
                <th>Status</th>
                <th>
                  <span className="sr-only">Record</span>
                </th>
              </tr>
            </thead>
            {days.map((day) => (
              <tbody key={day.key}>
                <tr className="audit-day">
                  <th colSpan={6}>{day.label}</th>
                </tr>
                {day.lines.map((line) => {
                  const key = keys.get(line) ?? String(line.ts_ms);
                  const isOpen = open.has(key);
                  const failed = auditFailed(line);
                  const deviceError = failed && line.status < 400;
                  const runId = line.tool === "runs.exec" ? runIdFromDetail(line.detail) : null;
                  const deviceName = line.device_id
                    ? deviceNames.get(line.device_id) ?? shortId(line.device_id)
                    : "—";
                  return (
                    <Fragment key={key}>
                      <tr className="audit-row">
                        <td className="audit-when" title={new Date(line.ts_ms).toISOString()}>
                          {age(line.ts_ms)}
                        </td>
                        <td title={line.device_id ?? undefined}>
                          {deviceName}
                          {line.caller === "console" && (
                            <>
                              {" "}
                              <ConsoleCaller className="audit-caller" />
                            </>
                          )}
                        </td>
                        <td>
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
                        <td className="audit-chevron-cell">
                          <button
                            type="button"
                            className="audit-disclosure"
                            aria-expanded={isOpen}
                            aria-label={`${isOpen ? "Hide" : "Show"} the record for ${
                              line.tool ?? line.kind
                            }`}
                            onClick={() => toggle(key)}
                          >
                            <span className="audit-chevron" aria-hidden="true">
                              {isOpen ? "▾" : "›"}
                            </span>
                          </button>
                        </td>
                      </tr>
                      {isOpen && (
                        <tr className="audit-record-row">
                          <td colSpan={6}>
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
                                {line.token_id ? tokenLabel(line.token_id, tokenNames) : "operator session"}
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
                            </dl>
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  );
                })}
              </tbody>
            ))}
          </table>
        </div>
      )}
    </div>
  );
}
