// useNow — ONE clock for every live duration on the panel.
//
// WHY IT EXISTS (round 62). Five components rendered an elapsed time as `Date.now() - startedAt` and re-rendered only
// when their props changed — and for a running command the props change when SSE delivers new OUTPUT. A silent
// command produces none, so the number FROZE: measured in `CommandCard.test.tsx`, five seconds of clock advanced the
// label from "517ms" to "517ms". The ledger already names that case in `sessionActive`'s own comment ("a long command
// that prints nothing for a while"), and the card was the one place that could not see it.
//
// THE BUDGET IS THE POINT, and it is why this is not a global ticker: it ticks ONCE A SECOND, and ONLY while the
// thing it measures is still moving. A panel that repaints every second with nothing running is exactly the idle
// repaint the objective forbids — so `active` is not an optimisation, it is half the contract, and `useNow.test.ts`
// pins both halves. Ended durations keep their exact value from the wire, because nothing re-renders them at all.
//
// The sub-second digits are honest but not smooth: the label shows the true elapsed time AT EACH TICK (1.2s, 2.3s,
// 3.1s), rather than being animated to look continuous. One repaint per second is the stated budget.
import { useEffect, useState } from "react";

const NOW_INTERVAL_MS = 1000;

export function useNow(active: boolean, intervalMs: number = NOW_INTERVAL_MS): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    // Read immediately, so the first paint after something starts is not up to a second stale.
    setNow(Date.now());
    const t = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(t);
  }, [active, intervalMs]);
  return now;
}