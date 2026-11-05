import { Fragment, useEffect, useMemo, useState } from "react";
import type { AuditLine, Device, TokenMeta } from "./api";
import { ACTIVITY_LIMIT, capSentence, windowLoadingSentence } from "./activityMerge";
import { auditFailed } from "./auditFilter";
import { ageAt, shortId } from "./format";
import { ConsoleCaller } from "./CallerMark";
import { tokenFact } from "./tokenFact";

/**
 * The activity as a record: what happened, when, on which machine.
 *
 * Outside, one line per action — the time, a sentence, the machine, the outcome. Inside (a click),
 * the evidence for that one call: the command as it was sent, the output tail, the session, and the
 * places to go and watch it. The list is the summary; the panel is the proof.
 *
 * A row fetched from the audit file has no evidence, because the file never carried arguments. It
 * says so rather than drawing an empty panel. A live row whose tool has no evidence template is a
 * different case: it is the call's own result, and the panel must not blame the audit file for what
 * the tool never offered.
 */

const DAY_NAMES = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

type Day = { key: string; label: string; lines: AuditLine[] };

function dayKey(ts: number): string {
  const date = new Date(ts);
  return `${date.getFullYear()}-${date.getMonth() + 1}-${date.getDate()}`;
}

function dayLabel(ts: number, now: number): string {
  if (dayKey(ts) === dayKey(now)) return "Today";
  if (dayKey(ts) === dayKey(now - 86_400_000)) return "Yesterday";
  const date = new Date(ts);
  const sameYear = date.getFullYear() === new Date(now).getFullYear();
  return `${DAY_NAMES[date.getDay()]}, ${date.getMonth() + 1}/${date.getDate()}${
    sameYear ? "" : `/${date.getFullYear()}`
  }`;
}

function clock(ts: number): string {
  const date = new Date(ts);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

/**
 * The row's clock, as a minute span and a seconds span.
 *
 * On a phone the time cell narrows to the minute; the seconds stay in their own element so the
 * stylesheet can drop them without dropping the instant — `dateTime` and the full text still carry
 * HH:MM:SS for anything that reads the record rather than looks at it.
 */
function Clock(props: { ts: number }) {
  const full = clock(props.ts);
  return (
    <time className="timeline-time" dateTime={new Date(props.ts).toISOString()}>
      <span className="timeline-clock">{full.slice(0, 5)}</span>
      <span className="timeline-clock-seconds">{full.slice(5)}</span>
    </time>
  );
}

/** Days newest first, each day's rows newest first (the list arrives that way). */
export function groupDays(lines: AuditLine[], now = Date.now()): Day[] {
  const days: Day[] = [];
  for (const line of lines) {
    const key = dayKey(line.ts_ms);
    const last = days[days.length - 1];
    if (last && last.key === key) last.lines.push(line);
    else days.push({ key, label: dayLabel(line.ts_ms, now), lines: [line] });
  }
  return days;
}

/**
 * The row's tone, which is the audit's own rule rather than a second opinion: a device's JSON-RPC
 * error is HTTP 200 and still a failure, and `ok: false` says so directly.
 */
function tone(line: AuditLine): "ok" | "bad" | "warn" {
  if (auditFailed(line)) return "bad";
  // The audit's generic failure marker, from before the status existed.
  if (line.detail === "error") return "warn";
  return "ok";
}

function machineName(devices: Device[], id: string | null | undefined): string | null {
  if (!id) return null;
  const device = devices.find((candidate) => candidate.id === id);
  return device?.name ?? shortId(id);
}

function text(evidence: Record<string, unknown>, key: string): string | null {
  const value = evidence[key];
  return typeof value === "string" && value !== "" ? value : null;
}

function list(evidence: Record<string, unknown>, key: string): string[] {
  const value = evidence[key];
  return Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === "string") : [];
}

/** The command as the caller wrote it, one string. */
function commandLine(evidence: Record<string, unknown>): string | null {
  const command = text(evidence, "command");
  if (!command) return null;
  const args = list(evidence, "args");
  return [command, ...args].join(" ");
}

const DETAIL_LABELS: Record<string, string> = {
  path: "Path",
  url: "URL",
  host: "Host",
  session: "Session",
  shell: "Shell",
  bytes: "Size",
  display: "Display",
  timeout_ms: "Timeout",
  query: "Query",
  exit_code: "Exit",
  timed_out: "Timed out"
};

/** One evidence key as a fact, or `null` when the value is not one this panel can show. */
function fact(evidence: Record<string, unknown>, key: string, label: string): readonly [string, string] | null {
  const value = evidence[key];
  if (typeof value === "string" && value !== "") return [label, value];
  if (typeof value === "number") {
    const shown = key === "bytes" ? `${value} B` : key === "timeout_ms" ? `${value} ms` : String(value);
    return [label, shown];
  }
  if (typeof value === "boolean") return [label, value ? "yes" : "no"];
  return null;
}

/**
 * The captured frame itself: the one piece of evidence an event cannot carry.
 *
 * The image is fetched lazily from the console's memory, behind the session the page already holds.
 * A load failure is a sentence, not a broken-image icon: the id may have been evicted, or the
 * console may have restarted since the row arrived.
 */
function CapturedFrame(props: { src: string; capturedAt: number }) {
  const [failed, setFailed] = useState(false);
  if (failed) {
    return (
      <p className="hint evidence-shot-missing">
        The captured frame could not be loaded; the console forgets old images when it restarts.
      </p>
    );
  }
  return (
    <figure className="evidence-shot">
      <img
        src={props.src}
        alt="The screen at the moment of the capture"
        loading="lazy"
        onError={() => setFailed(true)}
      />
      <figcaption>
        <span>Captured at {clock(props.capturedAt)}</span>
        <span className="evidence-shot-note">
          Kept in the console's memory; a restart forgets it.
        </span>
      </figcaption>
    </figure>
  );
}

/** The evidence panel for one row. */
function Evidence(props: {
  line: AuditLine;
  tokenNames: ReadonlyMap<string, string>;
  onWatch?: (deviceId: string) => void;
  onManageToken?: (tokenId: string) => void;
}) {
  const line = props.line;
  const evidence = line.evidence;
  const command = evidence ? commandLine(evidence) : null;
  const input = evidence ? text(evidence, "input") : null;
  const output = evidence ? text(evidence, "output") : null;
  const shot = evidence ? text(evidence, "shot") : null;
  const capturedAt =
    evidence && typeof evidence.captured_at_ms === "number" ? evidence.captured_at_ms : line.ts_ms;
  const facts = evidence
    ? Object.entries(DETAIL_LABELS)
        .map(([key, label]) => fact(evidence, key, label))
        .filter((entry): entry is readonly [string, string] => entry !== null)
    : [];
  const failed = auditFailed(line);
  const live = line.live === true;
  const detail = line.detail && line.detail !== "" ? line.detail : null;
  const canWatch = line.device_id && line.tool?.startsWith("terminal.");
  const empty = !command && !input && !output && !shot && facts.length === 0;

  return (
    <div className="evidence">
      <div className="evidence-main">
        {shot && <CapturedFrame src={shot} capturedAt={capturedAt} />}
        {command && (
          <>
            <p className="evidence-label">Command</p>
            <pre className="evidence-command">{command}</pre>
          </>
        )}
        {input && (
          <>
            <p className="evidence-label">Input</p>
            <pre className="evidence-command">{input}</pre>
          </>
        )}
        {output && (
          <>
            <p className="evidence-label">Output</p>
            <pre className="evidence-output">{output}</pre>
          </>
        )}
        {empty && (
          <p className="hint">
            {evidence
              ? "This call carried nothing the console can show."
              : live
                ? "This tool reports its result only."
                : "This row comes from the audit file, which never carries arguments — only the outcome. New calls open with their command and output."}
          </p>
        )}
      </div>
      <div className="evidence-side">
        <p className="evidence-label">Result</p>
        <dl className="evidence-facts">
          <div>
            <dt>Status</dt>
            <dd className={failed ? "is-bad" : "is-ok"}>{line.status || "—"}</dd>
          </div>
          {typeof line.ms === "number" && (
            <div>
              <dt>Took</dt>
              <dd>{line.ms} ms</dd>
            </div>
          )}
          {line.tool && (
            <div>
              <dt>Tool</dt>
              <dd className="mono" title={line.tool}>
                {line.tool}
              </dd>
            </div>
          )}
          <div>
            <dt>Token</dt>
            <dd>
              {tokenFact(line, props.tokenNames)}
              {line.token_id && props.onManageToken && (
                <button
                  type="button"
                  className="mini"
                  onClick={() => props.onManageToken?.(line.token_id as string)}
                >
                  Manage
                </button>
              )}
            </dd>
          </div>
          {detail && (
            <div>
              <dt>Detail</dt>
              <dd className={failed ? "is-bad" : undefined} title={detail}>
                {detail}
              </dd>
            </div>
          )}
          {facts.map(([label, value]) => (
            <div key={label}>
              <dt>{label}</dt>
              <dd
                className={label === "Path" || label === "Session" ? "mono" : undefined}
                title={value}
              >
                {value}
              </dd>
            </div>
          ))}
        </dl>
        {canWatch && (
          <button
            type="button"
            className="evidence-go"
            title="Opens this machine's terminal, newest session first"
            onClick={(event) => {
              event.stopPropagation();
              props.onWatch?.(line.device_id as string);
            }}
          >
            Open the terminal
          </button>
        )}
      </div>
    </div>
  );
}

/** One closed line, or one opened line with its evidence. */
/** What the stretch ending at `index - 1` held: how many calls, over how long. */
function stretchBefore(
  lines: readonly { device_id?: string | null; ts_ms: number }[],
  index: number
): {
  count: number;
  spanMs: number;
} {
  let first = index - 1;
  while (first > 0) {
    const before = lines[first - 1];
    const at = lines[first];
    if (before.device_id !== at.device_id || Math.abs(before.ts_ms - at.ts_ms) > 60_000) break;
    first -= 1;
  }
  return { count: index - first, spanMs: Math.abs(lines[index - 1].ts_ms - lines[first].ts_ms) };
}

/** A duration a person reads: 45 s, 2 m 10 s, 1 h 5 m. */
function spanText(spanMs: number): string {
  const total = Math.round(spanMs / 1000);
  if (total < 60) return `${total} s`;
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  if (minutes < 60) return seconds === 0 ? `${minutes} m` : `${minutes} m ${seconds} s`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours} h` : `${hours} h ${rest} m`;
}

function Row(props: {
  line: AuditLine;
  devices: Device[];
  tokenNames: ReadonlyMap<string, string>;
  open: boolean;
  stretch: boolean;
  nowMs: number;
  onToggle: () => void;
  onWatch?: (deviceId: string) => void;
  onManageToken?: (tokenId: string) => void;
}) {
  const line = props.line;
  const kind = tone(line);
  const machine = machineName(props.devices, line.device_id);
  // Only a row with something behind it is a control; a grant or a token row is a fact to read.
  const openable = Boolean(line.evidence || line.tool);
  const className =
    "timeline-row" + (props.open ? " is-open" : "") + (props.stretch ? " is-stretch" : "");
  const contents = (
    <>
      <Clock ts={line.ts_ms} />
      <span className="timeline-rail" aria-hidden="true">
        <span className="timeline-dot" data-tone={kind} />
      </span>
      <span className="timeline-words">
        <span className="timeline-sentence">{line.sentence ?? line.tool ?? line.kind}</span>
        <span className="timeline-facts">
          {machine && <span className="timeline-machine">{machine}</span>}
          {/* The console's own calls stay in the record; the quiet word is how a reader tells them
              from the AI's without opening the evidence. */}
          {line.caller === "console" && <ConsoleCaller className="timeline-caller" />}
          {typeof line.ms === "number" && line.ms > 0 && (
            <span className="timeline-ms">{line.ms} ms</span>
          )}
          {/* The outcome is a word, not the HTTP code: a device's JSON-RPC refusal is a failed call
              that crossed the console as 200, and "200" read as success. The code and the detail are
              still the evidence panel's facts for anyone who needs them. */}
          {kind !== "ok" && (
            <span className={"timeline-outcome is-" + kind} title={line.detail ?? undefined}>
              {kind === "bad" ? "failed" : "error"}
            </span>
          )}
          <span className="timeline-age">{ageAt(line.ts_ms, props.nowMs)}</span>
        </span>
      </span>
      {openable && (
        <span className={"timeline-chev" + (props.open ? " is-open" : "")} aria-hidden="true">
          ⌄
        </span>
      )}
    </>
  );

  return (
    <li className={className}>
      {openable ? (
        <button
          type="button"
          className="timeline-line"
          aria-expanded={props.open}
          onClick={props.onToggle}
        >
          {contents}
        </button>
      ) : (
        <div className="timeline-line is-static">{contents}</div>
      )}
      {props.open && (
        <Evidence
          line={line}
          tokenNames={props.tokenNames}
          onWatch={props.onWatch}
          onManageToken={props.onManageToken}
        />
      )}
    </li>
  );
}

/**
 * The machines the timeline can scope to: every registered machine, then any machine the lines
 * name that the fleet no longer lists, then the active filter if it is neither.
 *
 * A chip is a scope, not only a filter over the lines in hand: another machine's chip must stay
 * reachable while one machine's window is on screen. A machine with no line in this window still
 * gets a chip, and selecting it reads that machine's window instead of filtering this one.
 */
function machineOptions(
  lines: AuditLine[],
  devices: Device[],
  active: string
): Array<{ id: string; name: string; known: boolean }> {
  const present = new Map<string, { name: string; known: boolean }>();
  for (const device of devices) present.set(device.id, { name: device.name ?? device.id, known: true });
  for (const line of lines) {
    const id = line.device_id;
    if (!id || present.has(id)) continue;
    // A line can name an id the fleet does not list — an old enrollment, a retired machine. The
    // line is real, so it keeps a chip; the chip must not read like a machine.
    present.set(id, { name: `unknown machine · ${id.slice(0, 12)}`, known: false });
  }
  // The table can narrow to a machine this window has no line for — a retired device, or a window
  // that slid. The chip is how that filter stays visible (and clearable) on the timeline.
  if (active !== "" && !present.has(active)) {
    const known = devices.some((device) => device.id === active);
    present.set(active, {
      name: known ? machineName(devices, active) ?? active : `unknown machine · ${active.slice(0, 12)}`,
      known
    });
  }
  return Array.from(present, ([id, entry]) => ({ id, name: entry.name, known: entry.known }));
}

/**
 * The dated record: days, rows, the machine chips and one search.
 *
 * The timeline keeps only the filters it can honour. The scope toggle, the time window, the token
 * and the status describe columns of the table; here a row is narrowed by the machine that wrote it
 * and by what its sentence, tool, machine or detail says.
 */
export function ActivityTimeline(props: {
  lines: AuditLine[];
  devices: Device[];
  /** The operator's token registry, so a row can name a token instead of showing its id. */
  tokens?: TokenMeta[];
  onWatch?: (deviceId: string) => void;
  /** Open the Connect sheet on the token a record names. */
  onManageToken?: (tokenId: string) => void;
  empty?: string;
  /**
   * A fixed clock, for a page that is paused: the age column stops at this moment instead of
   * counting up, and the 1 s ticker that would move it is not started.
   */
  nowMs?: number;
  /** The search text, shared with the table's "Tool or detail" field. */
  query?: string;
  onQuery?: (value: string) => void;
  /** The machine the rows are narrowed to, or "" for every machine. */
  device?: string;
  onDevice?: (value: string) => void;
  /**
   * The machine chips and the search. The hub's timeline is already one machine's day, so it turns
   * them off rather than offering an All machines chip over one machine's lines.
   */
  showFilters?: boolean;
  /**
   * True when the list handed in was cut at [`ACTIVITY_LIMIT`] rather than being everything there
   * was. The page says so above the rows; the timeline alone cannot tell a full day from a capped
   * one, so it never guesses.
   */
  capped?: boolean;
  /**
   * The machine whose window was cut, when the window is one machine's; `null` for the fleet's.
   * The cap sentence names it, because the rows behind the cut are that machine's, not the fleet's.
   */
  cappedMachine?: string | null;
  /**
   * True when the fetched window came back with exactly the requested limit. This is not `capped`:
   * the page dropped nothing, but the server answered a full window and cannot prove an older line
   * exists. The sentence names the window's limit and claims nothing beyond it.
   */
  atLimit?: boolean;
  /**
   * True while a changed scope's window is being read. The rows still in hand belong to the previous
   * window, so the list shows one reading line instead of drawing another window's rows.
   */
  loading?: boolean;
  /** The machine whose window is being read, or `null` for the fleet's. */
  loadingMachine?: string | null;
}) {
  const [now, setNow] = useState(() => Date.now());
  const [openKey, setOpenKey] = useState<string | null>(null);
  // Uncontrolled when rendered on its own; the page passes the shared values so a search survives
  // the switch to the table and back.
  const [ownQuery, setOwnQuery] = useState("");
  const [ownDevice, setOwnDevice] = useState("");
  const filterable = props.showFilters !== false;
  const query = filterable ? (props.query ?? ownQuery) : "";
  const setQuery = props.onQuery ?? setOwnQuery;
  const device = filterable ? (props.device ?? ownDevice) : "";
  const setDevice = props.onDevice ?? setOwnDevice;
  const paused = props.nowMs !== undefined;
  useEffect(() => {
    if (paused) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [paused]);

  const at = props.nowMs ?? now;
  const needle = query.trim().toLowerCase();
  const tokenNames = useMemo(
    () => new Map((props.tokens ?? []).map((entry) => [entry.id, entry.name])),
    [props.tokens]
  );
  const machines = useMemo(
    () => machineOptions(props.lines, props.devices, device),
    [props.lines, props.devices, device]
  );
  const filtered = useMemo(() => {
    if (needle === "" && device === "") return props.lines;
    return props.lines.filter((line) => {
      if (device !== "" && line.device_id !== device) return false;
      if (needle === "") return true;
      const machine = line.device_id ? machineName(props.devices, line.device_id) : null;
      return [line.sentence, line.tool, line.detail, machine]
        .filter((value): value is string => typeof value === "string")
        .some((value) => value.toLowerCase().includes(needle));
    });
  }, [props.lines, props.devices, device, needle]);
  const days = groupDays(filtered, at);
  const filtering = needle !== "" || device !== "";
  const deviceLabel = device !== "" ? (machineName(props.devices, device) ?? device) : "";

  return (
    <div className="activity-timeline">
      {filterable && (
        <div className="timeline-filters">
          <div className="chips timeline-machines" role="group" aria-label="Filter by machine">
            <button
              type="button"
              className="chip"
              aria-pressed={device === ""}
              onClick={() => setDevice("")}
            >
              All machines
            </button>
            {machines.map((machine) => (
              <button
                key={machine.id}
                type="button"
                className={machine.known ? "chip" : "chip is-unknown"}
                title={machine.known ? machine.id : `not in the fleet: ${machine.id}`}
                aria-pressed={device === machine.id}
                onClick={() => setDevice(machine.id)}
              >
                {machine.name}
              </button>
            ))}
          </div>
          <label className="audit-field timeline-search">
            <span>Search activity</span>
            <input
              type="search"
              value={query}
              placeholder="sentence, tool, machine or detail"
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
        </div>
      )}
      {props.loading && (
        <p className="hint timeline-loading">
          {windowLoadingSentence(props.loadingMachine ?? null)}
        </p>
      )}
      {!props.loading && (props.capped || props.atLimit) && (
        // The fetched window and the live lines can add up to more than the console's cap. Dropping
        // the oldest is right; doing it silently would hide rows. The sentence names the window only:
        // the cut rows can be yesterday's, so "of today" would be false on a list that spans days.
        // A full request that dropped nothing is the other fact: the same helper says the window
        // hit its limit without pretending to know what is behind it.
        <p className="hint timeline-capped">
          {capSentence(
            ACTIVITY_LIMIT,
            props.cappedMachine ?? null,
            props.capped ? "capped" : "at-limit"
          )}
        </p>
      )}
      {!props.loading && (days.length === 0 ? (
        filtering ? (
          // A filter that hides every row is not "nothing happened": the page names what narrowed
          // the list and keeps the way back next to it.
          <div className="timeline-empty timeline-empty-filtered">
            <p className="hint">
              {needle !== ""
                ? `No activity matches “${query.trim()}”${device !== "" ? ` on ${deviceLabel}` : ""}.`
                : `No activity on ${deviceLabel}.`}
            </p>
            <div className="timeline-empty-actions">
              {needle !== "" && (
                <button type="button" onClick={() => setQuery("")}>
                  Clear search
                </button>
              )}
              {device !== "" && (
                <button type="button" onClick={() => setDevice("")}>
                  Clear machine filter
                </button>
              )}
            </div>
          </div>
        ) : (
          <p className="hint timeline-empty">
            {props.empty ??
              "No activity yet. Calls routed through this console are written here; a call made " +
                "straight to a machine's own endpoint is not. Connect a client sets up the " +
                "credential that routes one here."}
          </p>
        )
      ) : (
        <div className="timeline">
          {days.map((day) => (
            <section key={day.key} className="timeline-day" aria-label={day.label}>
              <h2 className="timeline-day-label is-sticky">{day.label}</h2>
              <ol className="timeline-rows">
                {day.lines.map((line, index) => {
                  const key = `${line.ts_ms}-${line.tool ?? line.kind}-${line.status}`;
                  // A minute of quiet, or a different machine, is where one stretch of work ends.
                  const previous = day.lines[index - 1];
                  const newStretch =
                    previous !== undefined &&
                    (previous.device_id !== line.device_id ||
                      Math.abs(previous.ts_ms - line.ts_ms) > 60_000);
                  // Where one stretch ends, say what it held. The boundary is derived from the same
                  // rule that draws the hairline: nothing is collapsed and no row is hidden.
                  const ended = newStretch ? stretchBefore(day.lines, index) : null;
                  return (
                    <Fragment key={`stretch-${key}`}>
                      {ended && (
                        <li className="timeline-stretch" aria-label="one stretch of work ends">
                          <span className="timeline-stretch-text">
                            {ended.count} {ended.count === 1 ? "call" : "calls"}
                            {ended.spanMs > 0 ? ` · ${spanText(ended.spanMs)}` : ""}
                          </span>
                        </li>
                      )}
                    <Row
                      key={key}
                      line={line}
                      devices={props.devices}
                      tokenNames={tokenNames}
                      open={openKey === key}
                      stretch={newStretch}
                      nowMs={at}
                      onToggle={() => setOpenKey((current) => (current === key ? null : key))}
                      onWatch={props.onWatch}
                      onManageToken={props.onManageToken}
                    />
                    </Fragment>
                  );
                })}
              </ol>
            </section>
          ))}
        </div>
      ))}
    </div>
  );
}
