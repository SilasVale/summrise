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

export type LiveToolsState = {
  tools: LiveTool[];
  /// A catalogue older than the cache window, or the last good one after a failed read.
  stale: boolean;
  /// The console's sentence (or the transport's) for the last failed read; null when it landed.
  error: string | null;
  /// True when the device itself is not connected, as opposed to a console or device tool error.
  offline: boolean;
  loading: boolean;
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
  const [tools, setTools] = useState<LiveTool[]>([]);
  const [stale, setStale] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [offline, setOffline] = useState(false);
  const [loading, setLoading] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const idRef = useRef<string | null>(deviceId);
  idRef.current = deviceId;

  useEffect(() => {
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
      })
      .catch((cause) => {
        if (!live || idRef.current !== deviceId) return;
        // The last good catalogue stays on screen, labelled stale. A failed read never renders as an
        // empty catalogue.
        setError(sentence(cause));
        setOffline(isOffline(cause));
        setStale(cache.has(deviceId));
      })
      .finally(() => {
        if (live && idRef.current === deviceId) setLoading(false);
      });
    return () => {
      live = false;
    };
  }, [deviceId, attempt]);

  const retry = useCallback(() => setAttempt((value) => value + 1), []);
  return { tools, stale, error, offline, loading, retry };
}
