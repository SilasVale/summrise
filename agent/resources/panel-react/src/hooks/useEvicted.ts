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
      // `void … .then(…)` AND NOT `async`: the parse is RUST NOW (P2, 2026-09-29), so it answers a
      // promise — and a listener that returned one would be handing it to an event dispatcher that
      // does nothing with it. The listener stays synchronous; the notice arrives one microtask
      // later, which is invisible for a frame that is already off the first render's path. The
      // timer is armed inside the callback so a frame this build REFUSES (`null`) never arms one.
      void parseEvicted((e as CustomEvent).detail).then((parsed) => {
        if (!parsed) return;
        setNotice(parsed);
        if (timer) clearTimeout(timer);
        timer = setTimeout(() => setNotice(null), ttlMs);
      });
    };
    window.addEventListener("summrise-session-evicted", onFrame);
    return () => {
      window.removeEventListener("summrise-session-evicted", onFrame);
      if (timer) clearTimeout(timer);
    };
  }, [ttlMs]);

  return notice;
}
