// ── THE PANEL'S ACK MECHANISM, PORTED — THE CONSOLE HAD NO EQUIVALENT ─────────────────────────────────────────────
//
// WHY THIS EXISTS (round 161 of the standing goal). Every acknowledgement finding the design sweep has produced is on the
// CONSOLE — `console overview: button.btn`, `console devices: button.btn.btn-ghost`, `console overview: button.rail-avatar`
// — and never on the panel. Reading both implementations is what showed the structural difference: the panel has had this
// hook since rounds 49-51 and spreads it onto every control, while the console had **one** local flag, added by hand in
// round 138, on the Overview's refresh button. **A mechanism on one surface and a habit on the other is how two surfaces
// come to behave differently under load.**
//
// THE TWO PROPERTIES THAT MATTER, both copied from `agent/resources/panel-react/src/lib/useAck.ts`:
//   1. IT FIRES ON THE EVENT. `setBusyOn(key)` runs in the same tick as the click, before the request leaves, so the
//      feedback does not wait for the network.
//   2. IT CLEARS ON EVERY EXIT, including a throw — `finally` is the only place that is true of.
//
// `aria-busy` travels with `data-busy` because they answer different readers: `data-busy` is what the design sweep reads
// (alongside `aria-busy` and `disabled`), and `aria-busy` is what assistive tech announces.
import { useCallback, useState } from "react";

interface Ack {
  /** The key of the control currently in flight, or null. */
  busyOn: string | null;
  /** True while ANY control in this component is in flight — what `disabled` wants. */
  busy: boolean;
  /** Run `fn` as the acknowledgement of `key`: sets the flag first, clears it on any exit. */
  run: (key: string, fn: () => Promise<unknown>) => Promise<void>;
  /** Spread onto the control: `{...ack("save")}`. */
  ack: (key: string) => { "aria-busy": true | undefined; "data-busy": "1" | undefined };
}

export function useAck(): Ack {
  const [busyOn, setBusyOn] = useState<string | null>(null);

  const run = useCallback(async (key: string, fn: () => Promise<unknown>) => {
    setBusyOn(key);
    try {
      await fn();
    } finally {
      // NOT a `catch`: a failed call is the caller's to report (each view has its own error surface), and swallowing it
      // here would hide a failure behind a cleared ring.
      setBusyOn(null);
    }
  }, []);

  const ack = useCallback(
    (key: string) => ({
      "aria-busy": busyOn === key || undefined,
      "data-busy": busyOn === key ? ("1" as const) : undefined,
    }),
    [busyOn],
  );

  return { busyOn, busy: busyOn !== null, run, ack };
}
