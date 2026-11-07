import { useCallback, useEffect, useRef, useState } from "react";
import { api, HttpError, type LiveTool } from "./api";

/**
 * The live catalogue is cached per device for 30 s and shared by Home, the Tools tab, Connect and
 * the token scope. The cache is module-level on purpose: those four surfaces mount at different times and
 * a per-hook cache would make each of them ask the device again.
 */
const CACHE_MS = 30_000;

type Cached = { tools: LiveTool[]; at: number };
const cache = new Map<string, Cached>();
const inFlight = new Map<string, Promise<Cached>>();

/** Test seam: the cache is shared state and a test must be able to start from a clean console. */
export function resetLiveToolsCache() {
  cache.clear();
  inFlight.clear();
}

/** The operator's last Retry, once the read it asked for settled. */
export type LiveToolsRetry = {
  /// Whether the retry reached the machine and read its catalogue.
  ok: boolean;
  /// When the read settled, in the console's own clock.
  at: number;
  /// The console's sentence for a failed read; null when it landed.
  error: string | null;
};

export type LiveToolsState = {
  tools: LiveTool[];
  /// A catalogue older than the cache window, or the last good one after a failed read.
  stale: boolean;
  /// The console's sentence (or the transport's) for the last failed read; null when it landed.
  error: string | null;
  /// True when the device itself is not connected, as opposed to a console or device tool error.
  offline: boolean;
  loading: boolean;
  /**
   * True only while the operator's Retry is in flight. The page's silent reads — mount, the five-
   * second poll, a recovered stream — never set it, so the button's working state is the press's.
   */
  retrying: boolean;
  /**
   * The operator's last Retry, after the read settled; null before the first press and after a
   * machine change. A failed retry names its outcome so the page can say "still offline" rather
   * than leave the press looking like the poll.
   */
  retried: LiveToolsRetry | null;
  retry: () => void;
};

function isOffline(cause: unknown): boolean {
  if (!(cause instanceof HttpError)) return false;
  return cause.status === 503 || cause.code === "device_offline" || cause.code === "offline";
}

function sentence(cause: unknown): string {
  if (cause instanceof HttpError) {
    if (cause.message && cause.message !== cause.code) return cause.message;
    if (isOffline(cause)) return "this machine is not connected; connect it, then try again";
  }
  return cause instanceof Error ? cause.message : String(cause);
}

function read(deviceId: string): Promise<Cached> {
  const pending = inFlight.get(deviceId);
  if (pending) return pending;
  const request = api
    .liveTools(deviceId)
    .then((answer) => {
      const value = { tools: Array.isArray(answer.tools) ? answer.tools : [], at: Date.now() };
      cache.set(deviceId, value);
      return value;
    })
    .finally(() => inFlight.delete(deviceId));
  inFlight.set(deviceId, request);
  return request;
}

/**
 * Read the selected machine's live `tools/list`, cached for 30 s. A failed read keeps the last good
 * catalogue for the session and marks it stale; a surface labels it instead of hiding it. `retry`
 * forces the next read.
 */
export function useLiveTools(deviceId: string | null): LiveToolsState {
  // **THE FIRST PAINT IS NOT A ZERO.** A device's read is on the wire from its first commit, so
  // `loading` starts true unless the cache already holds an answer. `useEffect` runs after paint,
  // so without this the hook's initial empty list is a zero a surface could draw before any read
  // answered (the measured 84T showed `0 tools · 0 capabilities` at first paint, audit-42 F5).
  const [tools, setTools] = useState<LiveTool[]>(() =>
    deviceId === null ? [] : cache.get(deviceId)?.tools ?? []
  );
  const [stale, setStale] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [offline, setOffline] = useState(false);
  const [loading, setLoading] = useState(() => deviceId !== null && !cache.has(deviceId));
  const [attempt, setAttempt] = useState(0);
  const [retrying, setRetrying] = useState(false);
  const [retried, setRetried] = useState<LiveToolsRetry | null>(null);
  const idRef = useRef<string | null>(deviceId);
  idRef.current = deviceId;
  // The machine whose operator pressed Retry. The read effect claims it once, so a machine change
  // that happens between the press and the effect cannot turn another machine's load into a retry.
  const operatorAttempt = useRef<string | null>(null);

  // A different machine is a different page: the last machine's press has no outcome here.
  useEffect(() => {
    setRetrying(false);
    setRetried(null);
  }, [deviceId]);

  useEffect(() => {
    const operator = deviceId !== null && operatorAttempt.current === deviceId;
    operatorAttempt.current = null;
    if (deviceId === null) {
      setTools([]);
      setStale(false);
      setError(null);
      setOffline(false);
      setLoading(false);
      return;
    }
    const cached = cache.get(deviceId);
    if (cached) {
      setTools(cached.tools);
      setStale(Date.now() - cached.at >= CACHE_MS);
    } else {
      setTools([]);
      setStale(false);
    }
    setError(null);
    setOffline(false);
    // `retry` is an explicit request: a fresh cache is not an answer when the operator asked again.
    if (attempt === 0 && cached && Date.now() - cached.at < CACHE_MS) {
      setLoading(false);
      return;
    }
    let live = true;
    setLoading(true);
    read(deviceId)
      .then((answer) => {
        if (!live || idRef.current !== deviceId) return;
        setTools(answer.tools);
        setStale(false);
        setError(null);
        setOffline(false);
        if (operator) {
          setRetried({ ok: true, at: Date.now(), error: null });
          setRetrying(false);
        }
      })
      .catch((cause) => {
        if (!live || idRef.current !== deviceId) return;
        // The last good catalogue stays on screen, labelled stale. A failed read never renders as an
        // empty catalogue.
        const sentenceForCause = sentence(cause);
        setError(sentenceForCause);
        setOffline(isOffline(cause));
        setStale(cache.has(deviceId));
        if (operator) {
          setRetried({ ok: false, at: Date.now(), error: sentenceForCause });
          setRetrying(false);
        }
      })
      .finally(() => {
        if (live && idRef.current === deviceId) setLoading(false);
      });
    return () => {
      live = false;
    };
  }, [deviceId, attempt]);

  const retry = useCallback(() => {
    const id = idRef.current;
    if (id === null) return;
    // The press is shown in the same render as the click: the button is disabled before the read
    // effect runs, so a fast second press cannot start a second attempt.
    operatorAttempt.current = id;
    setRetried(null);
    setRetrying(true);
    setAttempt((value) => value + 1);
  }, []);
  return { tools, stale, error, offline, loading, retrying, retried, retry };
}
