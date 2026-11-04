/**
 * Send terminal input as one MCP call, without letting a slow link stack keystrokes behind it.
 *
 * xterm's `onData` fires once per keystroke and once per chunk of a paste, and every chunk used to
 * become its own `terminal.write`: one MCP call, one audit line and one channel round trip per
 * character. The shell sees the same bytes either way.
 *
 * **THE TIMER WAS BUYING LATENCY, NOT AUDIT LINES.** It waited 25 ms hoping to merge keys, and a
 * person types a key every 100–200 ms, so it merged nothing for normal typing — it only added 25 ms
 * to the echo of every key. The merge it was written for is a *paste* (one `onData` chunk) and fast
 * typing while a write is in flight, and both are handled here without a timer: the first chunk goes
 * out at once, and anything typed while it is on the wire is accumulated and sent as one call when
 * it lands. Order is preserved (the shell must never see two writes swapped), and a burst becomes
 * one write because the previous one is still in flight, not because the sender waited.
 */
export type TerminalWriteBatcher = {
  push: (data: string) => void;
  flush: () => Promise<void>;
  cancel: () => void;
};

export function createTerminalWriteBatcher(
  send: (data: string) => Promise<void>,
  onError: (cause: unknown) => void
): TerminalWriteBatcher {
  let buffered = "";
  /** The write on the wire, if any. A chunk that arrives during it waits for this. */
  let inFlight: Promise<void> | null = null;
  /** A flush that was asked for while a write was in flight. */
  let pending = false;
  let cancelled = false;

  const drain = async (): Promise<void> => {
    const data = buffered;
    buffered = "";
    if (!data) return;
    try {
      await send(data);
    } catch (cause) {
      if (!cancelled) onError(cause);
    }
  };

  /** Start the next write, or note that one is owed. Never two at once. */
  const kick = () => {
    if (inFlight) {
      pending = true;
      return;
    }
    if (!buffered) return;
    pending = false;
    inFlight = drain().finally(() => {
      inFlight = null;
      // Text that arrived while this write was on the wire: one call, now.
      if (pending || buffered) kick();
    });
  };

  return {
    push(data: string) {
      if (cancelled) return;
      buffered += data;
      kick();
    },
    async flush() {
      // Everything typed so far leaves in as few calls as the wire allows: whatever is in flight
      // finishes, then one call carries what arrived behind it.
      while (inFlight) await inFlight;
      if (!buffered) return;
      kick();
      while (inFlight) await inFlight;
    },
    cancel() {
      cancelled = true;
      buffered = "";
      pending = false;
    }
  };
}
