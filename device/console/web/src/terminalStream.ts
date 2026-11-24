/**
 * The terminal's output as one open connection.
 *
 * The page used to ask for each chunk: a `terminal.read` long-poll per answer, which cost a request
 * per chunk and left the device with no reader parked between two of them. The console now serves a
 * server-sent stream per session, so one connection carries every chunk the device produces — the
 * shape the device's own panel has always had, and the reason its typing echo is one round trip
 * rather than one per keystroke.
 *
 * **THE CURSOR IS THE CALLER'S.** A frame says where the output ends (`cursor`), and a reconnect
 * asks from that offset, so a dropped connection replays exactly what was missed. That is also why
 * this never lets `EventSource` reconnect on its own: its automatic retry reuses the URL it was
 * built with, which would ask from a stale cursor and duplicate every byte since.
 */

export type StreamEnd = {
  exit_code?: number | null;
  signal?: string | null;
  cursor?: number;
};

export type StreamHandlers = {
  /** Bytes since the cursor, and the offset they end at. */
  output: (data: string, cursor: number) => void;
  /** The session is over; the facts are the device's own, when it gave them. */
  end: (facts: StreamEnd) => void;
  /** The device refused a read: the sentence names why, and it is the page's to show. */
  fault: (sentence: string, status: number | null) => void;
  /** The connection dropped, or the route is not there. The caller reconnects or falls back. */
  error: () => void;
};

export type TerminalStream = {
  close: () => void;
};

/** The slice of `EventSource` this module uses, so a test can drive it without a browser. */
export type EventSourceLike = {
  addEventListener: (type: string, listener: (event: MessageEvent) => void) => void;
  close: () => void;
  onerror: ((event: Event) => void) | null;
};

export type EventSourceFactory = (url: string) => EventSourceLike;

/** The route the console serves, with the session encoded into its two path segments. */
export function terminalStreamPath(deviceId: string, sessionId: string, from: number): string {
  const device = encodeURIComponent(deviceId);
  const session = encodeURIComponent(sessionId);
  return `/terminal/${device}/${session}/stream?from=${Math.max(0, Math.floor(from))}`;
}

/** `EventSource` where the page has one; jsdom has none, which is why the caller can fall back. */
export function defaultEventSourceFactory(): EventSourceFactory | null {
  const global = globalThis as { EventSource?: new (url: string) => EventSourceLike };
  if (typeof global.EventSource !== "function") return null;
  return (url) => new global.EventSource!(url);
}

/**
 * Open one session's stream.
 *
 * Returns `null` when the runtime has no `EventSource` — which is what a jsdom test has — so a
 * caller's fallback is the polling loop it already had, not a broken terminal.
 */
export function openTerminalStream(
  deviceId: string,
  sessionId: string,
  from: number,
  handlers: StreamHandlers,
  factory: EventSourceFactory | null = defaultEventSourceFactory()
): TerminalStream | null {
  if (!factory) return null;
  const source = factory(terminalStreamPath(deviceId, sessionId, from));
  let closed = false;

  const close = () => {
    if (closed) return;
    closed = true;
    source.close();
  };

  const onOutput = (event: MessageEvent) => {
    const frame = parse(event.data);
    if (!frame) return;
    const data = typeof frame.data === "string" ? frame.data : "";
    const cursor = typeof frame.cursor === "number" ? frame.cursor : null;
    if (data) handlers.output(data, cursor ?? from);
    else if (cursor !== null) handlers.output("", cursor);
  };
  const onEnd = (event: MessageEvent) => {
    const frame = parse(event.data) ?? {};
    close();
    handlers.end({
      exit_code: typeof frame.exit_code === "number" ? frame.exit_code : null,
      signal: typeof frame.signal === "string" ? frame.signal : null,
      cursor: typeof frame.cursor === "number" ? frame.cursor : undefined
    });
  };
  const onFault = (event: MessageEvent) => {
    const frame = parse(event.data) ?? {};
    close();
    handlers.fault(
      typeof frame.sentence === "string" ? frame.sentence : "the device refused the read",
      typeof frame.status === "number" ? frame.status : null
    );
  };
  source.addEventListener("output", onOutput);
  source.addEventListener("end", onEnd);
  source.addEventListener("fault", onFault);
  source.onerror = () => {
    if (closed) return;
    close();
    handlers.error();
  };

  return { close };
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
