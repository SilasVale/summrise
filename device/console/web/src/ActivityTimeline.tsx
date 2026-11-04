import { useEffect, useState } from "react";
import type { AuditLine, Device } from "./api";
import { age, shortId } from "./format";

/**
 * The activity as a record: what happened, when, on which machine.
 *
 * Outside, one line per action — the time, a sentence, the machine, the outcome. Inside (a click),
 * the evidence for that one call: the command as it was sent, the output tail, the session, and the
 * places to go and watch it. The list is the summary; the panel is the proof.
 *
 * A row fetched from the audit file has no evidence, because the file never carried arguments. It
 * says so rather than drawing an empty panel.
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

function tone(line: AuditLine): "ok" | "bad" | "warn" {
  if (line.status >= 400) return "bad";
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
  query: "Query"
};

/** The evidence panel for one row. */
function Evidence(props: { line: AuditLine; onWatch?: (deviceId: string) => void }) {
  const evidence = props.line.evidence;
  const command = evidence ? commandLine(evidence) : null;
  const input = evidence ? text(evidence, "input") : null;
  const output = evidence ? text(evidence, "output") : null;
  const facts = evidence
    ? Object.entries(DETAIL_LABELS)
        .map(([key, label]) => {
          const value = evidence[key];
          if (typeof value === "string" && value !== "") return [label, value] as const;
          if (typeof value === "number") {
            const shown =
              key === "bytes" ? `${value} B` : key === "timeout_ms" ? `${value} ms` : String(value);
            return [label, shown] as const;
          }
          return null;
        })
        .filter((entry): entry is readonly [string, string] => entry !== null)
    : [];
  const canWatch = props.line.device_id && props.line.tool?.startsWith("terminal.");

  if (!evidence && !output) {
    return (
      <div className="evidence evidence-quiet">
        <p className="hint">
          This row comes from the audit file, which never carries arguments — only the outcome. New
          calls open with their command and output.
        </p>
      </div>
    );
  }

  return (
    <div className="evidence">
      <div className="evidence-main">
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
        {!command && !input && !output && facts.length === 0 && (
          <p className="hint">This call carried nothing the console can show.</p>
        )}
      </div>
      <div className="evidence-side">
        <p className="evidence-label">Result</p>
        <dl className="evidence-facts">
          <div>
            <dt>Status</dt>
            <dd className={props.line.status >= 400 ? "is-bad" : "is-ok"}>
              {props.line.status || "—"}
            </dd>
          </div>
          {typeof props.line.ms === "number" && (
            <div>
              <dt>Took</dt>
              <dd>{props.line.ms} ms</dd>
            </div>
          )}
          {props.line.tool && (
            <div>
              <dt>Tool</dt>
              <dd className="mono">{props.line.tool}</dd>
            </div>
          )}
          {facts.map(([label, value]) => (
            <div key={label}>
              <dt>{label}</dt>
              <dd className={label === "Path" || label === "Session" ? "mono" : undefined}>{value}</dd>
            </div>
          ))}
        </dl>
        {canWatch && (
          <button
            type="button"
            className="evidence-go"
            onClick={(event) => {
              event.stopPropagation();
              props.onWatch?.(props.line.device_id as string);
            }}
          >
            Watch the terminal
          </button>
        )}
      </div>
    </div>
  );
}

/** One closed line, or one opened line with its evidence. */
function Row(props: {
  line: AuditLine;
  devices: Device[];
  open: boolean;
  onToggle: () => void;
  onWatch?: (deviceId: string) => void;
}) {
  const kind = tone(props.line);
  const machine = machineName(props.devices, props.line.device_id);
  const openable = Boolean(props.line.evidence || props.line.tool);
  return (
    <li className={"timeline-row" + (props.open ? " is-open" : "")}>
      <div
        className="timeline-line"
        role={openable ? "button" : undefined}
        tabIndex={openable ? 0 : undefined}
        aria-expanded={openable ? props.open : undefined}
        onClick={openable ? props.onToggle : undefined}
        onKeyDown={(event) => {
          if (!openable) return;
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            props.onToggle();
          }
        }}
      >
        <time className="timeline-time" dateTime={new Date(props.line.ts_ms).toISOString()}>
          {clock(props.line.ts_ms)}
        </time>
        <span className="timeline-rail" aria-hidden="true">
          <span className="timeline-dot" data-tone={kind} />
        </span>
        <span className="timeline-words">
          <span className="timeline-sentence">
            {props.line.sentence ?? props.line.tool ?? props.line.kind}
          </span>
          <span className="timeline-facts">
            {machine && <span className="timeline-machine">{machine}</span>}
            {typeof props.line.ms === "number" && props.line.ms > 0 && (
              <span className="timeline-ms">{props.line.ms} ms</span>
            )}
            {kind !== "ok" && (
              <span className={"timeline-outcome is-" + kind}>
                {props.line.status ? props.line.status : "error"}
              </span>
            )}
            <span className="timeline-age">{age(props.line.ts_ms)}</span>
          </span>
        </span>
        {openable && (
          <span className={"timeline-chev" + (props.open ? " is-open" : "")} aria-hidden="true">
            ⌄
          </span>
        )}
      </div>
      {props.open && <Evidence line={props.line} onWatch={props.onWatch} />}
    </li>
  );
}

/** The dated record: days, rows, and the evidence of the row that is open. */
export function ActivityTimeline(props: {
  lines: AuditLine[];
  devices: Device[];
  onWatch?: (deviceId: string) => void;
  empty?: string;
}) {
  const [now, setNow] = useState(() => Date.now());
  const [openKey, setOpenKey] = useState<string | null>(null);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  const days = groupDays(props.lines, now);
  if (days.length === 0) {
    return <p className="hint timeline-empty">{props.empty ?? "Nothing has happened yet."}</p>;
  }

  return (
    <div className="timeline">
      {days.map((day) => (
        <section key={day.key} className="timeline-day" aria-label={day.label}>
          <h2 className="timeline-day-label">{day.label}</h2>
          <ol className="timeline-rows">
            {day.lines.map((line, index) => {
              const key = `${line.ts_ms}-${line.tool ?? line.kind}-${line.status}`;
              // A minute of quiet, or a different machine, is where one stretch of work ends.
              const previous = day.lines[index - 1];
              const newStretch =
                previous !== undefined &&
                (previous.device_id !== line.device_id ||
                  Math.abs(previous.ts_ms - line.ts_ms) > 60_000);
              return (
                <div key={key} className={newStretch ? "timeline-stretch" : undefined}>
                  <Row
                    line={line}
                    devices={props.devices}
                    open={openKey === key}
                    onToggle={() => setOpenKey((current) => (current === key ? null : key))}
                    onWatch={props.onWatch}
                  />
                </div>
              );
            })}
          </ol>
        </section>
      ))}
    </div>
  );
}
