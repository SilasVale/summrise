/**
 * Batch terminal input into one MCP call.
 *
 * xterm's `onData` fires once per keystroke and once per chunk of a paste. Each chunk used to become
 * its own `terminal.write`: one MCP call, one audit line and one channel round trip per character.
 * The shell sees the same bytes either way, but the audit does not need a line per key. A 25 ms
 * buffer is below typing latency and above a paste's chunks, so a burst arrives as one write.
 */
export type TerminalWriteBatcher = {
  push: (data: string) => void;
  flush: () => Promise<void>;
  cancel: () => void;
};

export function createTerminalWriteBatcher(
  send: (data: string) => Promise<void>,
  onError: (cause: unknown) => void,
  delayMs = 25
): TerminalWriteBatcher {
  let buffered = "";
  let timer: number | undefined;
  let sent: Promise<void> = Promise.resolve();

  const flush = async () => {
    if (timer !== undefined) {
      window.clearTimeout(timer);
      timer = undefined;
    }
    const data = buffered;
    buffered = "";
    if (!data) return;
    try {
      await send(data);
    } catch (cause) {
      onError(cause);
    }
  };

  const push = (data: string) => {
    buffered += data;
    if (timer === undefined) {
      timer = window.setTimeout(() => {
        timer = undefined;
        sent = sent.then(flush);
      }, delayMs);
    }
  };

  const cancel = () => {
    if (timer !== undefined) {
      window.clearTimeout(timer);
      timer = undefined;
    }
    buffered = "";
  };

  return { push, flush, cancel };
}
