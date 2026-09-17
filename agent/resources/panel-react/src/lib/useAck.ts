// useAck — ACKNOWLEDGE THE CONTROL THAT WAS PRESSED, AND ACKNOWLEDGE IT ON THE EVENT.
//
// WHY THIS IS A HOOK AND NOT A HABIT. Twelve components in this panel keep their own `useState(false)` busy flag,
// and the flag says only that SOMETHING is in flight. The approval gate was the first to be fixed (round 247) and
// the fix was three lines long — but writing those three lines twelve times is how twelve behaviours happen. This
// is the one implementation:
//
//   const { busy, run, ack } = useAck();
//   <button disabled={busy} {...ack("search")} onClick={() => void run("search", doSearch)}>Search</button>
//
// THREE THINGS IT GETS RIGHT THAT A BOOLEAN DOES NOT:
//
//   1. IT NAMES THE CONTROL. `busyOn` is a key, so the control the operator pressed is the one that wears the
//      ring — its siblings only step back. A row where everything dims equally cannot answer "did my click
//      register?", which on a slow device is the only question the operator has.
//   2. IT FIRES ON THE EVENT. `setBusyOn(key)` runs in the same tick as the click, before the request leaves, so
//      the feedback does not wait for the network. That is the half of "responsive" that has nothing to do with
//      how fast the device is.
//   3. IT CLEARS ON EVERY EXIT, including a throw. `finally` is the only place that is true of.
//
// `aria-busy` travels with `data-busy` because they answer different readers: the attribute is what the paint
// keys on (`.ack-busy` in the sheet), and `aria-busy` is what assistive tech announces.
import { useCallback, useState } from "react";

export interface Ack {
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
      // NOT a `catch`: a failed call is the caller's to report (each of these components has its own error
      // surface), and swallowing it here would hide a failure behind a cleared ring.
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
