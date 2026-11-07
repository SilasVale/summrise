import { Fragment, useEffect, useMemo, useState } from "react";
import type { AuditLine, AuditSource, Device, TokenMeta } from "./api";
import {
  ACTIVITY_LIMIT,
  capSentence,
  windowLoadingSentence,
  type WindowCounts
} from "./activityMerge";
import { isChannelBoundary, isClientCall } from "./activitySummary";
import { auditFailed } from "./auditFilter";
import { spanText } from "./age";
import { ageAt, shortId } from "./format";
import { AuditSourceLine } from "./AuditSourceLine";
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

/**
 * How many calls a stretch must hold before its summary is drawn above its rows.
 *
 * Ten rows is about one screen at this density: below it the boundary label is still in view while
 * the rows it counts are, so moving it would announce a handful of rows rather than let a reader
 * skip them. The measured outlier was 470 rows / 25,850 px / 46% of the timeline — far past any
 * screen — and that is the one whose summary must arrive before the walk, not after it.
 */
const STRETCH_HEADER_ROWS = 10;

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

/**
 * Whether a value is shaped like a device id: the console mints them as 64 hex characters.
 *
 * A `device_id` is not always an id. A 403 `device_not_permitted` records the requested machine's
 * *name* in the field (audit-39 F2), so only this shape may be read as an id the fleet can lack.
 */
function isDeviceId(value: string): boolean {
  return /^[0-9a-f]{64}$/i.test(value);
}

/**
 * The fleet machine a recorded `device_id` names, by id or by name.
 *
 * A name the fleet knows is that machine, not an identity the fleet lacks: the live audit carries
 * `"device_id": "DESKTOP-84T3LIN"` for a machine whose fleet id is 64 hex.
 */
function fleetMachine(devices: Device[], value: string | null | undefined): Device | null {
  if (!value) return null;
  const byId = devices.find((device) => device.id === value);
  if (byId) return byId;
  const name = value.toLowerCase();
  return devices.find((device) => (device.name ?? "").toLowerCase() === name) ?? null;
}

/**
 * The machine a row belongs to: the fleet id when the record names the machine by id or by name,
 * else the recorded value itself (a retired id, or a name no machine carries).
 */
function machineKey(devices: Device[], value: string | null | undefined): string {
  if (!value) return "";
  return fleetMachine(devices, value)?.id ?? value;
}

function machineName(devices: Device[], id: string | null | undefined): string | null {
  if (!id) return null;
  const device = fleetMachine(devices, id);
  if (device) return device.name ?? device.id;
  return shortId(id);
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

/** One stretch of work: a run of rows on one machine, none more than a minute apart. */
type Stretch = { start: number; count: number; spanMs: number };

/**
 * The stretches of one day, newest first.
 *
 * A stretch begins where the machine changes or a minute passes — the same rule that draws the
 * hairline. Computing every stretch once lets the list know a brace of rows is long before it draws
 * the first of them; the old form could only count backwards from a boundary it had already passed.
 */
function dayStretches(lines: readonly AuditLine[]): Stretch[] {
  const stretches: Stretch[] = [];
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const previous = lines[index - 1];
    const begins =
      previous === undefined ||
      previous.device_id !== line.device_id ||
      Math.abs(previous.ts_ms - line.ts_ms) > 60_000;
    if (begins) {
      stretches.push({ start: index, count: 1, spanMs: 0 });
    } else {
      const stretch = stretches[stretches.length - 1];
      stretch.count += 1;
      // The list is newest first: the span runs from the stretch's first row to its newest.
      stretch.spanMs = Math.abs(line.ts_ms - lines[stretch.start].ts_ms);
    }
  }
  return stretches;
}

/** What a stretch held, in the one spelling both forms use. */
function stretchText(stretch: Stretch): string {
  const calls = `${stretch.count} ${stretch.count === 1 ? "call" : "calls"}`;
  return stretch.spanMs > 0 ? `${calls} · ${spanText(stretch.spanMs)}` : calls;
}

/** One closed line, or one opened line with its evidence. */
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
          {machine && (
            <span className="timeline-sep" aria-hidden="true">
              ·
            </span>
          )}
          <span className="timeline-age">{ageAt(line.ts_ms, props.nowMs)}</span>
          {/* The console's own calls stay in the record; the quiet word is how a reader tells them
              from the AI's without opening the evidence. */}
          {line.caller === "console" && <ConsoleCaller className="timeline-caller" />}
          {/* The outcome is a word, not the HTTP code: a device's JSON-RPC refusal is a failed call
              that crossed the console as 200, and "200" read as success. The code and the detail are
              still the evidence panel's facts for anyone who needs them. */}
          {kind !== "ok" && (
            <span className={"timeline-outcome is-" + kind} title={line.detail ?? undefined}>
              {kind === "bad" ? "failed" : "error"}
            </span>
          )}
          {typeof line.ms === "number" && line.ms > 0 && (
            <span className="timeline-ms">{line.ms} ms</span>
          )}
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

type MachineOption = { id: string; name: string; known: boolean; title: string };

/**
 * The machines the timeline can scope to: every registered machine, then any id the lines name
 * that the fleet no longer lists, then the active filter if it is neither.
 *
 * A chip is a scope, not only a filter over the lines in hand: another machine's chip must stay
 * reachable while one machine's window is on screen. A machine with no line in this window still
 * gets a chip, and selecting it reads that machine's window instead of filtering this one.
 *
 * A line's `device_id` is not always a machine id. A refusal records the requested machine's name
 * there (audit-39 F2), so a value that names a fleet machine is that machine's line and is folded
 * into its chip: the page must never call a machine that is in the fleet "not in the fleet". Only
 * a value shaped like an id (64 hex) that the fleet does not list keeps a chip of its own, with
 * what the record proves: `old id · <prefix> · <age of its newest line>`. A name no machine
 * carries is neither: it stays in the row's own record, because a name is not an id whose history
 * a scope can read. A filter the table set on such an id while this window has no line for it
 * keeps a chip without an age, so the filter stays visible and clearable without inventing a date.
 *
 * `goneId` is the one id that gets none: the page has already fallen back from it because `/fleet`
 * answered without it (audit-38 F8), and the page's fallback sentence is where that id is explained.
 * Its rows stay in the window — a removed machine's past activity is still the record — but a chip
 * here would present the selection the page just said is not in the fleet as a scope.
 */
function machineOptions(
  lines: AuditLine[],
  devices: Device[],
  active: string,
  goneId: string | null,
  nowMs: number
): MachineOption[] {
  const present = new Map<string, { name: string; known: boolean; newestMs: number | null }>();
  for (const device of devices) {
    present.set(device.id, { name: device.name ?? device.id, known: true, newestMs: null });
  }
  for (const line of lines) {
    const value = line.device_id;
    if (!value || value === goneId) continue;
    // The value names a fleet machine by its name: it already has that machine's chip. Skipping it
    // here is what keeps `old id · DESKTOP-84T3… · 37m ago` from sitting beside the live machine.
    const key = machineKey(devices, value);
    if (key !== value) continue;
    const entry = present.get(key);
    if (entry?.known) continue;
    if (entry) {
      if (entry.newestMs === null || line.ts_ms > entry.newestMs) entry.newestMs = line.ts_ms;
      continue;
    }
    // Only an id-shaped value can be a machine the fleet lost. A name is the record's own: drawing
    // it as an old id would invent a retired machine out of a refusal's device name.
    if (!isDeviceId(key)) continue;
    present.set(key, { name: "", known: false, newestMs: line.ts_ms });
  }
  // The table's device select offers the audit's retired ids too, so the shared filter can be an
  // id this window has no line for. Keep that filter visible and clearable, but claim no age: the
  // window has no line that could prove one. A name-shaped filter is left to the record like a
  // name-shaped line.
  if (active !== "" && active !== goneId && !present.has(active) && isDeviceId(active)) {
    present.set(active, { name: "", known: false, newestMs: null });
  }
  return Array.from(present, ([id, entry]) => {
    if (entry.known) {
      return { id, name: entry.name, known: true, title: id };
    }
    const age = entry.newestMs === null ? null : ageAt(entry.newestMs, nowMs);
    return {
      id,
      name: `old id · ${shortId(id)}${age === null ? "" : ` · ${age}`}`,
      known: false,
      title:
        age === null
          ? `not in the fleet: ${id}; no line in this window`
          : `not in the fleet: ${id}; newest line ${age}`
    };
  });
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
   * The selected id the page has fallen back from because `/fleet` answered without it (audit-38
   * F8). The id gets no machine chip — not from its lines, not as an active filter — because the
   * page's fallback sentence already says it is not in the fleet. Its rows still draw.
   */
  goneId?: string | null;
  /**
   * The "Clients only" control, shared with the page so the choice survives a view switch. Its count
   * is the fetched window's, labelled beside the control; the page header stays the day's sentence.
   * Uncontrolled when the timeline is rendered on its own.
   */
  clientsOnly?: boolean;
  onClientsOnly?: (next: boolean) => void;
  /**
   * The hub's one folded chip: the console's own reads in this day, counted and off by default.
   * The page owns the rows and the toggle; the timeline draws the control. Activity never passes
   * it, because the audit's default is every caller and its "Clients only" chip is the narrowing.
   */
  consoleReads?: { count: number; shown: boolean; onToggle: () => void };
  /**
   * The hub's third fold: the day's rows that belong to neither the AI nor the console. They are
   * real lines whose caller was never recorded, so the page counts them and offers them rather than
   * letting the day read as only the client's calls plus the console's reads. Activity never passes
   * it: there every caller is drawn by default.
   */
  unattributed?: { count: number; shown: boolean; onToggle: () => void };
  /**
   * The hub's own window sentence, when its controls are not the cap sentence's two counts. The
   * page owns the counts (three drawable buckets plus the withheld rows) and the fold, so it owns
   * the sentence; the timeline only draws it in the place the cap sentence goes.
   */
  windowSentence?: string | null;
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
   * What this view withheld from the fetched window: the drawn count, the fetch's count, and the
   * channel boundaries and repeated keys the merge did not draw. The cap sentence names them; a
   * view that has none of the facts keeps its simpler sentence.
   */
  windowCounts?: WindowCounts | null;
  /**
   * True while a changed scope's window is being read. The rows still in hand belong to the previous
   * window, so the list shows one reading line instead of drawing another window's rows.
   */
  loading?: boolean;
  /**
   * True when that read failed: the old scope's rows still may not be drawn under this scope, but
   * the list must not keep saying it is reading. The page's own notice states the failure.
   */
  loadingFailed?: boolean;
  /** The machine whose window is being read, or `null` for the fleet's. */
  loadingMachine?: string | null;
  /**
   * True when the page holds no audit window because its read is still out, or failed. The empty
   * sentence is a claim about the record ("No activity yet"), and a list that was not read cannot
   * make it; the page states the read instead. Live rows are still drawn, because they are real and
   * the page's notice says they are what it has.
   */
  unread?: boolean;
  /**
   * The file the window is read from, as `/audit/source` served it. The line beside the cap
   * sentence states it; absent or unreadable facts draw nothing rather than a guess.
   */
  source?: AuditSource | null;
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
  // The shared filter normally holds a fleet id, but it can also hold a name a refusal recorded in
  // `device_id`. Resolve it to the machine it names before the chips and rows ask what is selected
  // (audit-39 F2), so a name never becomes a second, retired-looking scope beside the machine.
  const scope = machineKey(props.devices, device);
  // **THE PAGE'S SUBJECT IS THE AI'S WORK.** One chip narrows the timeline to the calls a client can
  // be proven to have made (its caller, or a token on a row with no console mark), using the same
  // rule as the header's day count. It is a view control, not a scope: the window stays the one the
  // machine chips read, and the control's own count is how many rows it would draw from that
  // window. The header's `client calls` number is the day's and never this one.
  const [ownClientsOnly, setOwnClientsOnly] = useState(false);
  const clientsOnly = filterable && (props.clientsOnly ?? ownClientsOnly);
  const setClientsOnly = (next: boolean) => {
    if (props.onClientsOnly) props.onClientsOnly(next);
    else setOwnClientsOnly(next);
  };
  // **THE ROWS ARE CALLS, NOT CHANNEL FACTS.** `device.left` / `device.joined` are in the audit so a
  // fleet row can measure an absence, but a person watching the machine's work must not read them
  // as work. They are dropped before the chips, the counts and the days are derived.
  const rows = useMemo(
    () => props.lines.filter((line) => !isChannelBoundary(line)),
    [props.lines]
  );
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
    () => machineOptions(rows, props.devices, scope, props.goneId ?? null, at),
    [rows, props.devices, scope, props.goneId, at]
  );
  // The rows the machine and search filters keep, before the caller filter. The "Clients only"
  // control counts this same set: its number is the rows it would draw in the fetched window, not
  // the day's client-call number above (nineteenth item 5's two windows).
  const matching = useMemo(() => {
    if (needle === "" && scope === "") return rows;
    return rows.filter((line) => {
      if (scope !== "" && machineKey(props.devices, line.device_id) !== scope) return false;
      if (needle === "") return true;
      const machine = line.device_id ? machineName(props.devices, line.device_id) : null;
      return [line.sentence, line.tool, line.detail, machine]
        .filter((value): value is string => typeof value === "string")
        .some((value) => value.toLowerCase().includes(needle));
    });
  }, [rows, props.devices, scope, needle]);
  const filtered = useMemo(
    () => (clientsOnly ? matching.filter(isClientCall) : matching),
    [matching, clientsOnly]
  );
  const clientCalls = useMemo(() => matching.filter(isClientCall).length, [matching]);
  const days = groupDays(filtered, at);
  const filtering = needle !== "" || scope !== "" || clientsOnly;
  const deviceLabel = scope !== "" ? (machineName(props.devices, scope) ?? scope) : "";

  return (
    <div className="activity-timeline">
      {/* THE HUB IS THE AI'S WORK; THE CONSOLE'S READS ARE ONE CHIP AWAY. The page hands in the
          rows it drew, so this is only the control: it says how many console rows are folded and
          adds them when pressed. It is rendered even when the list is empty, so an empty hub names
          what is hidden instead of offering nothing to reach it. */}
      {props.consoleReads && (
        <div className="chips timeline-console-reads">
          <button
            type="button"
            className="chip"
            aria-pressed={props.consoleReads.shown}
            title="Add the console's own reads to this list; they are not an AI client's calls"
            onClick={props.consoleReads.onToggle}
          >
            Console reads · {props.consoleReads.count}
          </button>
        </div>
      )}
      {/* THE ROWS NOBODY CAN ATTRIBUTE GET A CONTROL, NOT A SILENCE. Activity's header has always
          counted them; the hub used to fold the day to the client's calls and leave them out of
          every count. The chip's number is that same bucket. It is offered only when there is one:
          a zero is already stated by Activity's header and would be noise on every machine. */}
      {props.unattributed && props.unattributed.count > 0 && (
        <div className="chips timeline-unattributed">
          <button
            type="button"
            className="chip"
            aria-pressed={props.unattributed.shown}
            title="Add the lines whose caller was never recorded; they are neither the AI's calls nor the console's reads"
            onClick={props.unattributed.onToggle}
          >
            Unattributed · {props.unattributed.count}
          </button>
        </div>
      )}
      {filterable && (
        <div className="timeline-filters">
          <div className="filter-block">
            <span className="filter-label" id="timeline-machines-label">
              Machines
            </span>
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
                  className={machine.known ? "chip" : "chip is-old"}
                  title={machine.title}
                  aria-pressed={scope === machine.id}
                  onClick={() => setDevice(machine.id)}
                >
                  {machine.name}
                </button>
              ))}
            </div>
          </div>
          <div className="filter-block filter-block-callers">
            <span className="filter-label" id="timeline-callers-label">
              Callers
            </span>
            <div
              className="chips timeline-callers"
              role="group"
              aria-labelledby="timeline-callers-label"
            >
              <button
                type="button"
                className="chip"
                aria-pressed={clientsOnly}
                aria-describedby="timeline-clients-window"
                title="Only calls a client made: the caller says so, or the row carries a token"
                onClick={() => setClientsOnly(!clientsOnly)}
              >
                Clients only
              </button>
              <span className="filter-count" id="timeline-clients-window">
                {clientCalls} {clientCalls === 1 ? "row" : "rows"} in this window
              </span>
            </div>
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
      {props.loading && !props.loadingFailed && (
        <p className="hint timeline-loading">
          {windowLoadingSentence(props.loadingMachine ?? null)}
        </p>
      )}
      {!props.loading && (days.length === 0 ? (
        props.unread ? null : filtering ? (
          // A filter that hides every row is not "nothing happened": the page names what narrowed
          // the list and keeps the way back next to it.
          <div className="timeline-empty timeline-empty-filtered">
            <p className="hint">
              {clientsOnly
                ? `No client calls${needle !== "" ? ` match “${query.trim()}”` : ""}${
                    device !== "" ? ` on ${deviceLabel}` : ""
                  } in this window.`
                : needle !== ""
                  ? `No activity matches “${query.trim()}”${device !== "" ? ` on ${deviceLabel}` : ""}.`
                  : `No activity on ${deviceLabel}.`}
            </p>
            <div className="timeline-empty-actions">
              {clientsOnly && (
                <button type="button" onClick={() => setClientsOnly(false)}>
                  Show every caller
                </button>
              )}
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
          {days.map((day) => {
            const stretches = dayStretches(day.lines);
            const orderAt = new Map(stretches.map((stretch, order) => [stretch.start, order]));
            return (
              <section key={day.key} className="timeline-day" aria-label={day.label}>
                <h2 className="timeline-day-label is-sticky">{day.label}</h2>
                <ol className="timeline-rows">
                  {day.lines.map((line, index) => {
                    const key = `${line.ts_ms}-${line.tool ?? line.kind}-${line.status}`;
                    const order = orderAt.get(index);
                    const current = order === undefined ? null : stretches[order];
                    const previous = order !== undefined && order > 0 ? stretches[order - 1] : null;
                    // A stretch worth more than a screen says what it is before its first row.
                    const header =
                      current !== null && current.count > STRETCH_HEADER_ROWS ? current : null;
                    // Everything shorter keeps the form it had: the label arrives where the stretch
                    // ends. One call is not a stretch; its row already says what happened.
                    const footer =
                      previous !== null &&
                      previous.count > 1 &&
                      previous.count <= STRETCH_HEADER_ROWS
                        ? previous
                        : null;
                    const newStretch = order !== undefined && order > 0;
                    return (
                      <Fragment key={`stretch-${key}`}>
                        {footer && (
                          <li className="timeline-stretch" aria-label="one stretch of work ends">
                            <span className="timeline-stretch-text">{stretchText(footer)}</span>
                          </li>
                        )}
                        {header && (
                          <li
                            className="timeline-stretch is-header"
                            aria-label="the stretch below begins"
                          >
                            <span className="timeline-stretch-text">{stretchText(header)}</span>
                          </li>
                        )}
                        <Row
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
            );
          })}
        </div>
      ))}
      {/* The technical hints are the record's footer, not its preface: what was drawn and what was
          read are facts to consult after the day, not a banner above it. They stay quiet and last. */}
      {!props.loading && (
        <footer className="timeline-footer">
          {(props.windowSentence != null || props.capped || props.atLimit) && (
            // The fetched window and the live lines can add up to more than the console's cap.
            // Dropping the oldest is right; doing it silently would hide rows. The sentence names
            // the window only: the cut rows can be yesterday's, so "of today" would be false on a
            // list that spans days. A full request that dropped nothing is the other fact: the same
            // helper says the window hit its limit without pretending to know what is behind it.
            <p className="hint timeline-capped">
              {props.windowSentence ??
                capSentence(
                  ACTIVITY_LIMIT,
                  props.cappedMachine ?? null,
                  props.capped ? "capped" : "at-limit",
                  props.windowCounts ?? null
                )}
            </p>
          )}
          <AuditSourceLine source={props.source} />
        </footer>
      )}
    </div>
  );
}
