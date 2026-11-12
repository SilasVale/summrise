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
 *
 * A page can be slower than the console's broadcast buffer without dropping the connection. The
 * console then sends one `lagged` frame carrying the number of lines the buffer discarded, and the
 * page has a hole it cannot recover from the stream itself: the caller must read `/audit` again.
 * This module keeps that number; filling the hole is the caller's job.
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
  /**
   * The permanent token record that authenticated a client's call, by id. The audit line already
   * carries it; the live event carries it too, because the newest rows win the merge and a line that
   * dropped it would read "—" where the record names the token.
   */
  token_id?: string;
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

/**
 * One call the console has announced and not yet cleared: a `started` frame.
 *
 * It is deliberately not a [`LiveLine`]: it has no sentence, no status and no place in the record.
 * The proxy publishes it before the wait and clears it when the wait ends, so a page can say a call
 * is running while the audit still has nothing about it.
 */
export type StartedCall = {
  /** The proxy's id for this call; a `finished` frame clears exactly this one. */
  id: string;
  /** The tool the call named. */
  tool: string;
  /** The console's clock when the call began. */
  at_ms: number;
  /** The machine the call names, when it names one. */
  device_id?: string;
  /** Who made the call, so the band can count the AI and not the console's own reads. */
  caller?: "console" | "client";
};

/**
 * The facts a device event's payload can carry that the page reads.
 *
 * The payload is the event kind's own: a session names the session it opened, and a
 * `device.started` names its port. Only the session's id is drawn here, so only the fields the page
 * can vouch for are typed rather than inventing one shape for every kind.
 */
export type DevicePayload = {
  /** The session's id, on `session.opened` and `session.closed`. */
  id?: string;
  /** How the session reaches its machine: `pty`, `ssh` or `serial`. */
  kind?: string;
};

/**
 * One event from a device's own journal, as the console relays it on the stream.
 *
 * The console does not rewrite the device's envelope: `v`, `plane`, `seq`, `ts` and `type` are the
 * journal's own fields, `subject` is whatever the event names as its subject, and `payload` is the
 * kind's facts. A frame that does not carry these fields is not an event the page can report.
 */
export type DeviceEvent = {
  /** The envelope's schema version; additive changes stay at 1. */
  v: number;
  /** Which plane the event belongs to; the console's own events are `control`. */
  plane: "control" | "data";
  /** The device journal's monotonic sequence for this event. */
  seq: number;
  /** When the device recorded it, as an RFC 3339 string. */
  ts: string;
  /** `session.opened`, `session.closed`, `device.started`, … */
  type: string;
  /** What the event is about, when it names a subject. */
  subject?: unknown;
  /** The facts the event kind carries. */
  payload: DevicePayload;
};

/**
 * One reset of a device's journal: the rows this page still needs are no longer retained.
 *
 * The console sends it when the journal has moved past the point this page's record resumes from.
 * `snapshot_seq` is where the journal now stands, and `lost` is the inclusive range of sequence
 * numbers the page can no longer read. The stream cannot refill them, so the caller has to reread the
 * record — the same answer a `lagged` frame needs.
 */
export type DeviceReset = {
  /** The device whose journal reset. */
  device: string;
  /** The journal's sequence the snapshot resumed at. */
  snapshot_seq: number;
  /** The inclusive range of sequence numbers the journal no longer retains. */
  lost: { from: number; to: number };
};

/**
 * The proxy's own cut on one call: the longest a `started` entry may stand without a `finished`.
 *
 * The proxy stops waiting at ninety seconds for a run and sooner for everything else. A page that
 * never hears the clear must not keep "running" forever, so it applies the same bound itself; the
 * console's snapshot applies it too, which is what keeps a reconnect from resurrecting the entry.
 */
export const RUNNING_CUT_MS = 90_000;

/** The started calls that have not outlived the proxy's cut at `now`. */
export function runningAt(calls: readonly StartedCall[], now: number): StartedCall[] {
  return calls.filter((call) => now - call.at_ms <= RUNNING_CUT_MS);
}

export type LiveHandlers = {
  /**
   * The newest lines, oldest first, replacing whatever the page had, and the calls still running.
   * A reconnect's snapshot is the registry's whole list, so it also replaces the running list.
   */
  snapshot: (lines: LiveLine[], running?: StartedCall[]) => void;
  /** One line, written after the connection opened. */
  activity: (line: LiveLine) => void;
  /** The page fell behind: how many lines the console's buffer dropped before this event. */
  lagged: (dropped: number) => void;
  /** One call began: the proxy announced it before the device answered. */
  started?: (call: StartedCall) => void;
  /** One call's reply arrived (or its proxy stopped waiting); clear the line by id. */
  finished?: (id: string) => void;
  /**
   * One event from a device's own journal. It is not an audit line and not a call: the event has a
   * `seq` of its own and no HTTP status, so the page reads the envelope rather than the record.
   */
  device?: (device: string, event: DeviceEvent) => void;
  /**
   * A device's journal no longer retains rows this page needs. The inclusive `lost` range is a hole
   * the stream cannot refill, so the caller must reread the record as it would after `lagged`.
   */
  reset?: (reset: DeviceReset) => void;
  /** The connection dropped. `EventSource` retries on its own. */
  error: () => void;
  /**
   * A snapshot arrived after the connection had dropped: the stream is re-describing its history,
   * and whatever was written while it was down is not in the old list. Optional because a test or a
   * runtime may not care; the snapshot itself is still delivered.
   */
  recovered?: () => void;
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
  // The connection failed since the last snapshot. A snapshot that follows is a reconnect, not a
  // routine refresh: the page is told once so it can reread what the down time hid.
  let dropped = false;

  const close = () => {
    if (closed) return;
    closed = true;
    source.close();
  };

  const onSnapshot = (event: MessageEvent) => {
    const frame = parse(event.data);
    const lines = frame && Array.isArray(frame.lines) ? frame.lines : [];
    const running =
      frame && Array.isArray(frame.running)
        ? frame.running
            .map((entry) => asStarted(entry, undefined))
            .filter((call): call is StartedCall => call !== null)
        : [];
    handlers.snapshot(
      lines.map(asLine).filter((line): line is LiveLine => line !== null),
      running
    );
    if (dropped) {
      dropped = false;
      handlers.recovered?.();
    }
  };
  const onActivity = (event: MessageEvent) => {
    const line = asLine(parse(event.data));
    if (line) handlers.activity(line);
  };
  const onLagged = (event: MessageEvent) => {
    // A lagged frame is always a hole, even if its body cannot be counted. Saying one rather than
    // zero keeps a malformed body from reading as "nothing was lost".
    const dropped = droppedCount(parse(event.data));
    handlers.lagged(dropped);
  };
  const onStarted = (event: MessageEvent) => {
    const frame = parse(event.data);
    const call = frame ? asStarted(frame.started, frame.caller) : null;
    if (call) handlers.started?.(call);
  };
  const onFinished = (event: MessageEvent) => {
    const frame = parse(event.data);
    const id = finishedId(frame);
    if (id !== null) handlers.finished?.(id);
  };
  const onDevice = (event: MessageEvent) => {
    const parsed = asDeviceEvent(parse(event.data));
    if (parsed) handlers.device?.(parsed.device, parsed.event);
  };
  const onReset = (event: MessageEvent) => {
    const reset = asReset(parse(event.data));
    if (reset) handlers.reset?.(reset);
  };
  source.addEventListener("snapshot", onSnapshot);
  source.addEventListener("activity", onActivity);
  source.addEventListener("lagged", onLagged);
  source.addEventListener("started", onStarted);
  source.addEventListener("finished", onFinished);
  source.addEventListener("device", onDevice);
  source.addEventListener("reset", onReset);
  source.onerror = () => {
    if (closed) return;
    dropped = true;
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
    ...(typeof frame.token_id === "string" && frame.token_id !== ""
      ? { token_id: frame.token_id }
      : {}),
    ...(typeof frame.ms === "number" ? { ms: frame.ms } : {}),
    ...(frame.evidence && typeof frame.evidence === "object"
      ? { evidence: frame.evidence as Record<string, unknown> }
      : {})
  };
}

/**
 * One started call, or `null` when the payload does not carry the facts the band needs.
 *
 * The `started` object of a live frame and an entry in a snapshot's `running` array are the same
 * facts; the caller is top-level on the frame and inner on the snapshot entry, so the caller is a
 * parameter rather than assumed.
 */
function asStarted(value: unknown, caller: unknown): StartedCall | null {
  if (!value || typeof value !== "object") return null;
  const frame = value as Record<string, unknown>;
  const id = typeof frame.id === "string" && frame.id !== "" ? frame.id : null;
  const tool = typeof frame.tool === "string" && frame.tool !== "" ? frame.tool : null;
  const at = typeof frame.at_ms === "number" && Number.isFinite(frame.at_ms) ? frame.at_ms : null;
  if (id === null || tool === null || at === null) return null;
  const audience =
    caller === "console" || caller === "client"
      ? caller
      : frame.caller === "console" || frame.caller === "client"
        ? frame.caller
        : undefined;
  return {
    id,
    tool,
    at_ms: at,
    ...(typeof frame.device_id === "string" ? { device_id: frame.device_id } : {}),
    ...(audience ? { caller: audience } : {})
  };
}

/**
 * One relayed device event, or `null` when the frame does not carry an envelope the page can
 * report.
 *
 * The console does not write a sentence for these — the device's event is the whole fact — so a
 * half-read envelope is refused rather than drawn with blanks. The wrapper is `device_event`
 * because the console answers `device` too: the device's id is outside the journal line, and the
 * frame must not confuse which machine wrote it.
 */
function asDeviceEvent(
  frame: Record<string, unknown> | null
): { device: string; event: DeviceEvent } | null {
  const wrapper = frame?.device_event;
  if (!wrapper || typeof wrapper !== "object" || Array.isArray(wrapper)) return null;
  const relayed = wrapper as Record<string, unknown>;
  const device =
    typeof relayed.device === "string" && relayed.device !== "" ? relayed.device : null;
  const raw = relayed.event;
  if (device === null || !raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const event = raw as Record<string, unknown>;
  const v = typeof event.v === "number" && Number.isFinite(event.v) ? event.v : null;
  const plane = event.plane === "control" || event.plane === "data" ? event.plane : null;
  const seq = typeof event.seq === "number" && Number.isFinite(event.seq) ? event.seq : null;
  const ts = typeof event.ts === "string" && event.ts !== "" ? event.ts : null;
  const type = typeof event.type === "string" && event.type !== "" ? event.type : null;
  if (v === null || plane === null || seq === null || ts === null || type === null) return null;
  return {
    device,
    event: {
      v,
      plane,
      seq,
      ts,
      type,
      ...(event.subject !== undefined ? { subject: event.subject } : {}),
      payload: asDevicePayload(event.payload)
    }
  };
}

/** The payload's facts the page reads. Every other field belongs to the event kind, not to this page. */
function asDevicePayload(value: unknown): DevicePayload {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const raw = value as Record<string, unknown>;
  return {
    ...(typeof raw.id === "string" ? { id: raw.id } : {}),
    ...(typeof raw.kind === "string" ? { kind: raw.kind } : {})
  };
}

/**
 * One journal reset, or `null` when the frame does not name the machine, the sequence that resumed,
 * and the range that is gone.
 *
 * Unlike a relayed device event, this frame is flat: the reset is the whole payload. A frame missing
 * any of its three facts is refused, because a page cannot report a gap it cannot size.
 */
function asReset(frame: Record<string, unknown> | null): DeviceReset | null {
  if (!frame) return null;
  const device = typeof frame.device === "string" && frame.device !== "" ? frame.device : null;
  const snapshotSeq =
    typeof frame.snapshot_seq === "number" && Number.isFinite(frame.snapshot_seq)
      ? frame.snapshot_seq
      : null;
  const lost = frame.lost;
  if (
    device === null ||
    snapshotSeq === null ||
    !lost ||
    typeof lost !== "object" ||
    Array.isArray(lost)
  ) {
    return null;
  }
  const range = lost as Record<string, unknown>;
  const from = typeof range.from === "number" && Number.isFinite(range.from) ? range.from : null;
  const to = typeof range.to === "number" && Number.isFinite(range.to) ? range.to : null;
  if (from === null || to === null) return null;
  return { device, snapshot_seq: snapshotSeq, lost: { from, to } };
}

/** The id a `finished` frame clears, or `null` when it names none. */
function finishedId(frame: Record<string, unknown> | null): string | null {
  const finished = frame?.finished;
  if (!finished || typeof finished !== "object") return null;
  const id = (finished as Record<string, unknown>).id;
  return typeof id === "string" && id !== "" ? id : null;
}

/**
 * The number of lines a `lagged` frame says the server dropped.
 *
 * The console always sends a positive count. A body that arrived without one still came from a lag
 * the console announced, so the answer is one rather than zero: the page may undercount an unknown
 * hole, but it must not report it as no hole at all.
 */
function droppedCount(frame: Record<string, unknown> | null): number {
  const dropped = frame?.dropped;
  if (typeof dropped === "number" && Number.isFinite(dropped) && dropped > 0) {
    return Math.max(1, Math.floor(dropped));
  }
  return 1;
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
