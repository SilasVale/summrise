// The most recent eviction the device announced, held just long enough to be read.
//
// ONE NOTICE, SELF-EXPIRING. Like the monitor banners, this is the device speaking first about
// something that already happened, so it must not accumulate: a device at its session cap evicting
// on every open would otherwise stack lines forever. A newer eviction replaces the older one, and
// the notice leaves on its own — nothing here needs dismissing to keep working.
import { useEffect, useState } from "react";
import { parseEvicted, type EvictionNotice } from "../lib/evicted";

/** Long enough to notice and read one line, short enough not to linger over the strip. */
const EVICTED_TTL_MS = 20_000;

export function useEvictedNotice(ttlMs: number = EVICTED_TTL_MS): EvictionNotice | null {
  const [notice, setNotice] = useState<EvictionNotice | null>(null);

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const onFrame = (e: Event) => {
      const parsed = parseEvicted((e as CustomEvent).detail);
      if (!parsed) return;
      setNotice(parsed);
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => setNotice(null), ttlMs);
    };
    window.addEventListener("summrise-session-evicted", onFrame);
    return () => {
      window.removeEventListener("summrise-session-evicted", onFrame);
      if (timer) clearTimeout(timer);
    };
  }, [ttlMs]);

  return notice;
}
