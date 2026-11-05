import { useCallback, useEffect, useRef, useState } from "react";
import { openLiveActivity, runningAt, type LiveLine, type StartedCall } from "./liveActivity";

/** How many lines the page keeps: the hub reads five, Activity filters a few hundred. */
export const LIVE_LIMIT = 200;

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
 * what falls off is older than anything the page draws.
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
      lagged: (dropped) => {
        setLagged((current) => current + dropped);
        void catchUp();
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
