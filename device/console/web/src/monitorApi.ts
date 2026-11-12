import { callTool, ToolError, type Grant } from "./terminal";

/**
 * **WHAT THIS MACHINE WATCHES, READ THE WAY THE DEVICE REPORTS IT (the hundred-and-seventy-second list).**
 *
 * `monitor.list` answers `{ok, interval_secs, targets:[{id, host, port, path, summary, series}]}`
 * (`agent/src/plugins/monitor/tools.rs`), and each `summary` is `monitor::summary_of`'s payload:
 * `{probes, up, down, up_pct, up_now, since_ms, latency, drops, last_status, last_expect_ok}` — every one
 * of which is `null` until a probe exists. **A null is not a zero**: "never probed" and "probed, all down"
 * are different facts about a service, and the page draws them as different words.
 */
export type MonitorSummary = {
  probes: number;
  up: number;
  down: number;
  /** Percent of probes that were up, or null before the first probe. */
  upPct: number | null;
  /** Whether the newest probe was up, or null before the first probe. */
  upNow: boolean | null;
  /** When the current run of that state began, in Unix milliseconds. */
  sinceMs: number | null;
  /** The last HTTP status, when the target is watched with a path. */
  lastStatus: number | null;
  /** How many times it fell from up to down inside the window. */
  drops: number | null;
};

export type MonitorTarget = {
  id: string;
  host: string;
  port: number;
  path: string | null;
  summary: MonitorSummary;
};

export type MonitorRead = { targets: MonitorTarget[]; intervalSecs: number | null };

const num = (value: unknown): number | null =>
  typeof value === "number" && Number.isFinite(value) ? value : null;

const summary = (value: unknown): MonitorSummary => {
  const raw = (value !== null && typeof value === "object" ? value : {}) as Record<string, unknown>;
  return {
    probes: num(raw.probes) ?? 0,
    up: num(raw.up) ?? 0,
    down: num(raw.down) ?? 0,
    upPct: num(raw.up_pct),
    upNow: typeof raw.up_now === "boolean" ? raw.up_now : null,
    sinceMs: num(raw.since_ms),
    lastStatus: num(raw.last_status),
    drops: num(raw.drops)
  };
};

/** The targets, or a sentence naming what the machine answered instead. */
export async function readMonitors(grant: Grant): Promise<MonitorRead> {
  const body = await callTool(grant, "monitor.list", {});
  let answer: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(body);
    // A bare array is one of the two shapes (see below), so it is kept as an array and read by `Array.isArray` there.
    answer = (parsed !== null && typeof parsed === "object" ? parsed : {}) as Record<string, unknown>;
  } catch {
    throw new Error("the machine answered with text, not a list of services");
  }
  if (answer.ok === false) {
    const refusal = typeof answer.error === "string" ? answer.error : "";
    throw new Error(refusal !== "" ? refusal : "the machine refused the read");
  }
  // **TWO SHAPES, AND THE DEVICE SAYS WHICH BY ANSWERING.** Measured on the LIVE fleet 2026-11-12: `monitor.list` on
  // DESKTOP-9VH92BJ (0.3.292) answers **`[]`** — a bare array — while the newer source in this tree answers
  // `{ok, interval_secs, targets: […]}`. The page read only the newer one, so a machine with nothing to watch reported
  // `the machine answered monitor.list without a list` (caught in the live check, not by a test — which is why the live check
  // exists). Both are read here, and neither is guessed: an answer that is neither is still an error.
  const list: unknown[] | null = Array.isArray(answer)
    ? answer
    : Array.isArray(answer.targets)
      ? answer.targets
      : null;
  if (list === null) {
    throw new Error("the machine answered monitor.list without a list");
  }
  const targets = list.flatMap((value): MonitorTarget[] => {
    const raw = (value !== null && typeof value === "object" ? value : {}) as Record<string, unknown>;
    const host = typeof raw.host === "string" ? raw.host : "";
    const port = num(raw.port);
    // A target without an address cannot be named in a sentence, so it is dropped rather than drawn blank.
    if (host === "" || port === null) return [];
    return [
      {
        id: typeof raw.id === "string" ? raw.id : `${host}:${port}`,
        host,
        port,
        path: typeof raw.path === "string" && raw.path !== "" ? raw.path : null,
        summary: summary(raw.summary)
      }
    ];
  });
  return { targets, intervalSecs: num(answer.interval_secs) };
}

/** The page's sentence for a failed read: the device's words when it gave them, this page's otherwise. */
export function monitorFailure(error: unknown): string {
  if (error instanceof ToolError) {
    return error.message.trim() !== ""
      ? `The services could not be read — ${error.message.trim()}`
      : "The services could not be read.";
  }
  return error instanceof Error && error.message.trim() !== ""
    ? `The services could not be read — ${error.message.trim()}`
    : "The services could not be read.";
}
