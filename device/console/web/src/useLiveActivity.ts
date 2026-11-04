import { useCallback, useEffect, useState } from "react";
import { openLiveActivity, type LiveLine } from "./liveActivity";

/** How many lines the page keeps: the hub reads five, Activity filters a few hundred. */
export const LIVE_LIMIT = 200;

export type LiveFeedState = {
  /** Newest first. */
  lines: LiveLine[];
  /** `live` once a snapshot has arrived; `offline` when the connection is down. */
  state: "connecting" | "live" | "offline";
  /** Drop the connection and open it again (each connection opens with a fresh snapshot). */
  retry: () => void;
};

/**
 * Follow the console's activity for as long as this page is open.
 *
 * A snapshot replaces the list — it is the audit's own newest lines — and each later event is put in
 * front of it. The list is capped, because a page left open for a day must not grow without bound;
 * what falls off is older than anything the page draws.
 */
export function useLiveActivity(enabled: boolean): LiveFeedState {
  const [lines, setLines] = useState<LiveLine[]>([]);
  const [state, setState] = useState<"connecting" | "live" | "offline">("connecting");
  const [attempt, setAttempt] = useState(0);

  const retry = useCallback(() => {
    setState("connecting");
    setAttempt((current) => current + 1);
  }, []);

  useEffect(() => {
    if (!enabled) return;
    const feed = openLiveActivity({
      // The snapshot is oldest first; the page draws newest first.
      snapshot: (list) => {
        setLines(list.slice(-LIVE_LIMIT).reverse());
        setState("live");
      },
      activity: (line) => {
        setLines((current) => [line, ...current].slice(0, LIVE_LIMIT));
        setState("live");
      },
      lagged: () => setState("connecting"),
      error: () => setState("offline")
    });
    if (!feed) {
      setState("offline");
      return;
    }
    return () => feed.close();
  }, [enabled, attempt]);

  return { lines, state, retry };
}
