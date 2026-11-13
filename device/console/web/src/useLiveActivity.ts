import { useCallback, useEffect, useRef, useState } from "react";
import {
  openLiveActivity,
  runningAt,
  type DeviceEvent,
  type LiveLine,
  type StartedCall
} from "./liveActivity";

/** How many lines the page keeps: the hub reads five, Activity filters a few hundred. */
export const LIVE_LIMIT = 200;

/**
 * The sentence a device event earns on Activity.
 *
 * The device's journal carries no human sentence, and `session.opened on <machine>` would make the
 * reader learn the journal's own spelling. The two session events name the terminal they are
 * about, and a started device names itself; a call's outcome is described by [`callSentence`], which
 * needs the tool the event named. Anything else is reported as the event's type, because the page
 * knows nothing about that kind beyond the fact that it happened on this machine.
 */
function deviceSentence(device: string, event: DeviceEvent): string {
  if (event.type === "session.opened" || event.type === "session.closed") {
    const id = event.payload.id;
    if (id !== undefined && id !== "") {
      const verb = event.type === "session.opened" ? "opened" : "closed";
      return `terminal ${id} ${verb} on ${device}`;
    }
  }
  if (event.type === "device.started") return `${device} started`;
  return `${event.type} on ${device}`;
}

/**
 * The sentence one call's outcome earns: what ran, and how it ended.
 *
 * A call the machine made on its own reaches the page as a device event, and `call.denied on
 * <machine>` would make it read as a journal kind rather than as a call. The reason is the device's
 * own code, shown as it wrote it rather than translated into a claim the page cannot keep; without
 * one the parenthetical is left off instead of promising a fact the device did not supply.
 */
function callSentence(
  device: string,
  type: "call.finished" | "call.denied",
  tool: string,
  reason: string | undefined
): string {
  if (type === "call.finished") return `${tool} ran on ${device}`;
  return reason === undefined
    ? `${tool} was refused on ${device}`
    : `${tool} was refused on ${device} (${reason})`;
}

/**
 * One device event as a row, or `null` when the event is a half that earns none.
 *
 * A session fact is not a call: the envelope has no HTTP status, and the device's `ts` is its own
 * clock — a body whose time cannot be read falls back to this page's, which is newer than the
 * event, never older.
 *
 * A call's journal pair is one call, so only its outcome half is a row. `call.started` is the half
 * the paired `call.finished`/`call.denied` describes, and drawing both would show one call twice —
 * once with no outcome, once with. A started event whose outcome never comes is not worth a row
 * either: the page must not invent a timer to guess when a call died, and an unfinished half with
 * no answer is not a fact it can report.
 */
function deviceLine(device: string, event: DeviceEvent): LiveLine | null {
  if (event.type === "call.started") return null;
  const parsed = Date.parse(event.ts);
  const ts_ms = Number.isFinite(parsed) ? parsed : Date.now();
  if (event.type === "call.finished" || event.type === "call.denied") {
    const tool = event.payload.tool;
    // The parser refuses an outcome that names no tool, so this guard only narrows the type; a row
    // without one would be the blank the parser already rejected before the page saw it.
    if (tool === undefined) return null;
    // The console records a tool-level failure as 424, so a call the machine refused reads failed
    // with every other failed call in the list rather than as a green 200.
    const ok = event.payload.ok === true;
    return {
      ts_ms,
      kind: "mcp",
      status: ok ? 200 : 424,
      ok,
      device_id: device,
      tool,
      ...(event.trace?.call !== undefined ? { trace: event.trace.call } : {}),
      live: true,
      sentence: callSentence(device, event.type, tool, event.payload.reason)
    };
  }
  return {
    ts_ms,
    kind: "session",
    status: 200,
    ok: true,
    device_id: device,
    live: true,
    sentence: deviceSentence(device, event)
  };
}

export type LiveFeedState = {
  /** Newest first. */
  lines: LiveLine[];
  /**
   * Calls the proxy has announced and not cleared, oldest first. Not a line and not in the audit:
   * the band draws it above the last steps and drops it at the proxy's cut.
   */
  running: StartedCall[];
  /** `live` once a snapshot has arrived; `offline` when the connection is down. */
  state: "connecting" | "live" | "offline";
  /**
   * Lines the console's buffer dropped that no reread has covered yet. Cumulative across dropped
   * frames; zero only after `reread` has answered for every one of them.
   */
  lagged: number;
  /**
   * How many snapshots have arrived after the connection dropped. Each one is a reconnect: the
   * snapshot replaces the list, and anything written while the stream was down is a hole the page
   * must reread. The first snapshot opens the stream and does not count.
   */
  recoveries: number;
  /** Drop the connection and open it again (each connection opens with a fresh snapshot). */
  retry: () => void;
};

/**
 * Follow the console's activity for as long as this page is open.
 *
 * A snapshot replaces the list — it is the audit's own newest lines — and each later event is put in
 * front of it. The list is capped, because a page left open for a day must not grow without bound;
 * what falls off is older than anything the page draws. A device event is put in front the same way,
 * as a session row: it is not a call, so it never joins `running`.
 *
 * A `lagged` frame means the console's broadcast buffer overflowed under this tab: the lines between
 * the last one drawn and the one the stream resumed with are a hole, not a pause. `reread` is called
 * to fetch the audit window again, and `lagged` holds the count until that answer lands — so a page
 * that shows the count cannot call the record complete while the hole is still there. A normal line
 * arriving in between is only the stream resuming; it does not prove the hole was filled.
 *
 * `reread` is optional because a runtime may have no audit route to read. Without it the count stays
 * up rather than falling back to a silence that would look like completeness.
 *
 * A snapshot after `error` replaces the list and is counted in `recoveries`: the lines written while
 * the stream was down are not in it, so a page that draws a fetched window beside the stream has a
 * hole to reread. The opening snapshot is not a recovery.
 */
export function useLiveActivity(
  enabled: boolean,
  reread?: () => Promise<unknown>
): LiveFeedState {
  const [lines, setLines] = useState<LiveLine[]>([]);
  const [running, setRunning] = useState<StartedCall[]>([]);
  const [state, setState] = useState<"connecting" | "live" | "offline">("connecting");
  const [lagged, setLagged] = useState(0);
  const [recoveries, setRecoveries] = useState(0);
  const [attempt, setAttempt] = useState(0);
  // The connection effect must not reopen because the caller passed a new closure; the ref lets each
  // lag use the latest reader while the stream stays one connection.
  const rereadRef = useRef(reread);
  rereadRef.current = reread;

  const retry = useCallback(() => {
    setState("connecting");
    setAttempt((current) => current + 1);
  }, []);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    let rereading = false;
    let again = false;

    /**
     * Read the audit window for the hole, and do not lower `lagged` until the read actually answers.
     * A second drop while a read is in flight schedules one more read: the first answer was read
     * before that drop was known, so it cannot be the answer that clears it.
     */
    const catchUp = async () => {
      if (rereading) {
        again = true;
        return;
      }
      const read = rereadRef.current;
      if (!read) return;
      rereading = true;
      do {
        again = false;
        try {
          await read();
        } catch {
          // No answer landed. The count stays so the page keeps saying the record has a hole.
          rereading = false;
          return;
        }
      } while (again && !cancelled);
      rereading = false;
      if (!cancelled) setLagged(0);
    };

    /**
     * Note a hole in the record and ask for the reread that fills it.
     *
     * `lagged` and `reset` are the same fact from two planes: the console's buffer dropped lines, or
     * a device's journal no longer retains them. Both counts are cumulative until a reread answers,
     * so both take this one path.
     */
    const noteGap = (lost: number) => {
      setLagged((current) => current + lost);
      void catchUp();
    };

    const feed = openLiveActivity({
      // The snapshot is oldest first; the page draws newest first. A reconnect's running list is
      // the console's registry, so it replaces this page's copy rather than adding to it.
      snapshot: (list, inFlight) => {
        setLines(list.slice(-LIVE_LIMIT).reverse());
        setRunning(inFlight ?? []);
        setState("live");
      },
      activity: (line) => {
        setLines((current) => [line, ...current].slice(0, LIVE_LIMIT));
        // A busy page prunes here as well as in the band: a clear that was dropped costs the entry
        // its place the next time anything arrives, not only when the line above is drawn.
        setRunning((current) => runningAt(current, Date.now()));
        setState("live");
      },
      started: (call) => {
        setRunning((current) =>
          runningAt(
            [call, ...current.filter((entry) => entry.id !== call.id)],
            Date.now()
          )
        );
        setState("live");
      },
      finished: (id) => {
        setRunning((current) => current.filter((entry) => entry.id !== id));
      },
      // A device event is the machine's own journal reaching the page. The console filters out the
      // calls it proxied itself, so what lands here is a session fact or a call made from the
      // machine's own panel, a mesh peer or loopback — work that would otherwise be invisible. It
      // earns a row so that call is visible as it happens, and stays out of `running`, which is the
      // proxy's in-flight calls and nothing else; a half that earns no row — a call still waiting
      // for its outcome — adds none.
      device: (device, event) => {
        const row = deviceLine(device, event);
        if (row === null) {
          setState("live");
          return;
        }
        setLines((current) => [row, ...current].slice(0, LIVE_LIMIT));
        setState("live");
      },
      lagged: noteGap,
      // A journal reset is a hole the stream cannot refill: the rows between this page's record and
      // the device's snapshot are gone. It takes the lagged path — count the missing rows, reread
      // the record — and leaves `lines` alone, because only the reread's snapshot replaces them.
      reset: (reset) => {
        noteGap(Math.max(0, reset.lost.to - reset.lost.from + 1));
      },
      // A lag is a healthy connection that lost lines, not a lost connection: the state stays
      // `live`, and `lagged` is what tells the page the record has a hole.
      error: () => setState("offline"),
      // A reconnect's snapshot replaces the list. A page drawing a fetched window and the stream
      // together has a hole between them until the window is reread; `recoveries` is what says a
      // reread is owed.
      recovered: () => setRecoveries((current) => current + 1)
    });
    if (!feed) {
      setState("offline");
      return;
    }
    return () => {
      cancelled = true;
      feed.close();
    };
  }, [enabled, attempt]);

  return { lines, running, state, lagged, recoveries, retry };
}
