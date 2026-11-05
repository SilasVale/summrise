/**
 * The console's activity as one open connection.
 *
 * The page used to draw "Recent calls" from a fetch, so a call that happened after the last refresh
 * was invisible until the next one — there was no sense of an AI working *now*. The console serves
 * the same audit as server-sent events: a `snapshot` of the newest lines, then one `activity` event
 * per line as it is written. Every line carries a `sentence` the console wrote, so two pages cannot
 * describe one call differently.
 *
 * Reconnecting is safe here, unlike the terminal stream: each connection opens with a fresh snapshot,
 * which replaces whatever the page had. That is why `EventSource`'s own retry is left in charge — a
 * cursor would be needed only if the stream could not re-describe its history.
 */

/** One activity line, as `/events` sends it. */
export type LiveLine = {
  /** The console's clock for the action, in Unix milliseconds. */
  ts_ms: number;
  /** `mcp`, `grant`, `token`, `register`, `enroll`, `denied`. */
  kind: string;
  /** The machine the action names, when it names one. */
  device_id?: string;
  /** The HTTP status the caller saw. */
  status: number;
  /** Who made the call: the console's own read, or an AI client's. Absent on an older line. */
  caller?: "console" | "client";
  /** The tool a `tools/call` named. */
  tool?: string;
  /** Why, for a refusal. */
  detail?: string;
  /** Whether the device's answer was a success. */
  ok?: boolean;
  /**
   * Whether this line was watched live. A line in a `snapshot` was read back from the audit file and
   * does not carry it; only an event published as it happened does.
   */
  live?: boolean;
  /** How long the device took, when the console measured it. */
  ms?: number;
  /** The human sentence the console wrote for this event. */
  sentence: string;
  /** What the row shows when it is opened: the call's own arguments, bounded. */
  evidence?: Record<string, unknown>;
};

export type LiveHandlers = {
  /** The newest lines, oldest first, replacing whatever the page had. */
  snapshot: (lines: LiveLine[]) => void;
  /** One line, written after the connection opened. */
  activity: (line: LiveLine) => void;
  /** The page fell behind; the caller refetches or waits for the next snapshot. */
  lagged: () => void;
  /** The connection dropped. `EventSource` retries on its own. */
  error: () => void;
};

export type LiveFeed = {
  close: () => void;
};

/** The slice of `EventSource` this module uses, so a test can drive it without a browser. */
export type EventSourceLike = {
  addEventListener: (type: string, listener: (event: MessageEvent) => void) => void;
  close: () => void;
  onerror: ((event: Event) => void) | null;
};

export type EventSourceFactory = (url: string) => EventSourceLike;

/** The route the console serves. */
export function liveActivityPath(): string {
  return "/events";
}

/** `EventSource` where the page has one; jsdom has none, which is why the caller can fall back. */
export function defaultEventSourceFactory(): EventSourceFactory | null {
  const global = globalThis as { EventSource?: new (url: string) => EventSourceLike };
  if (typeof global.EventSource !== "function") return null;
  return (url) => new global.EventSource!(url);
}

/**
 * Open the activity stream.
 *
 * Returns `null` when the runtime has no `EventSource`, so a caller keeps the fetch it already had
 * instead of showing an empty stream as if nothing were happening.
 */
export function openLiveActivity(
  handlers: LiveHandlers,
  factory: EventSourceFactory | null = defaultEventSourceFactory()
): LiveFeed | null {
  if (!factory) return null;
  const source = factory(liveActivityPath());
  let closed = false;

  const close = () => {
    if (closed) return;
    closed = true;
    source.close();
  };

  const onSnapshot = (event: MessageEvent) => {
    const frame = parse(event.data);
    const lines = frame && Array.isArray(frame.lines) ? frame.lines : [];
    handlers.snapshot(lines.map(asLine).filter((line): line is LiveLine => line !== null));
  };
  const onActivity = (event: MessageEvent) => {
    const line = asLine(parse(event.data));
    if (line) handlers.activity(line);
  };
  const onLagged = () => handlers.lagged();
  source.addEventListener("snapshot", onSnapshot);
  source.addEventListener("activity", onActivity);
  source.addEventListener("lagged", onLagged);
  source.onerror = () => {
    if (closed) return;
    handlers.error();
  };

  return { close };
}

/** One line, or `null` when the payload is not one. A sentence is what makes it readable. */
function asLine(value: unknown): LiveLine | null {
  if (!value || typeof value !== "object") return null;
  const frame = value as Record<string, unknown>;
  const ts = typeof frame.ts_ms === "number" ? frame.ts_ms : null;
  const sentence = typeof frame.sentence === "string" ? frame.sentence : null;
  if (ts === null || !sentence) return null;
  return {
    ts_ms: ts,
    kind: typeof frame.kind === "string" ? frame.kind : "mcp",
    status: typeof frame.status === "number" ? frame.status : 0,
    sentence,
    ...(typeof frame.device_id === "string" ? { device_id: frame.device_id } : {}),
    ...(typeof frame.tool === "string" ? { tool: frame.tool } : {}),
    ...(typeof frame.detail === "string" ? { detail: frame.detail } : {}),
    ...(typeof frame.ok === "boolean" ? { ok: frame.ok } : {}),
    ...(typeof frame.live === "boolean" ? { live: frame.live } : {}),
    ...(frame.caller === "console" || frame.caller === "client"
      ? { caller: frame.caller }
      : {}),
    ...(typeof frame.ms === "number" ? { ms: frame.ms } : {}),
    ...(frame.evidence && typeof frame.evidence === "object"
      ? { evidence: frame.evidence as Record<string, unknown> }
      : {})
  };
}

function parse(text: unknown): Record<string, unknown> | null {
  if (typeof text !== "string" || text === "") return null;
  try {
    const value = JSON.parse(text);
    return value && typeof value === "object" ? (value as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}
