/**
 * What reaches xterm, one frame at a time.
 *
 * `terminal.read` answers with everything the device still retains — up to its whole one-mebibyte
 * ring — and a busy session answers that way several times a second. One `term.write` of a megabyte
 * is a synchronous parse and layout on the main thread, which reads as a frozen page. The legacy
 * panel hit this on adopted sessions and fixed it with a per-frame pump; the console writes the
 * answer straight through, so this is that pump, kept in its own module because the rule is about
 * the display and not about the terminal's protocol.
 */

/** The largest string handed to xterm in one frame. */
export const WRITE_SLICE_CHARS = 64 * 1024;

/**
 * Cut `text` into slices of at most `size` UTF-16 units without splitting a surrogate pair.
 *
 * `String.prototype.slice` counts UTF-16 units, so an arbitrary cut can leave half of an emoji or any
 * non-BMP character at the end of one slice and half at the start of the next — a lone surrogate,
 * which renders as a replacement character. When a cut lands on the low half of a pair, the boundary
 * moves back one unit so the pair stays together.
 */
export function splitWriteSlices(text: string, size: number = WRITE_SLICE_CHARS): string[] {
  if (size <= 0) {
    return [text];
  }
  const slices: string[] = [];
  let start = 0;
  while (start < text.length) {
    let end = Math.min(start + size, text.length);
    if (end < text.length && end > start) {
      const code = text.charCodeAt(end);
      // A low surrogate here means `end - 1` is a high surrogate: keep the pair together.
      if (code >= 0xdc00 && code <= 0xdfff) {
        end -= 1;
      }
    }
    slices.push(text.slice(start, end));
    start = end;
  }
  return slices;
}

/** What a terminal's output queue accepts. */
export type OutputPump = {
  /** Queue text for the next frame; empty text is ignored. */
  push: (text: string) => void;
  /** Drop everything queued and cancel the scheduled frame, for a tab that is going away. */
  cancel: () => void;
};

const nextFrame = (callback: () => void): number =>
  typeof requestAnimationFrame === "function"
    ? requestAnimationFrame(callback)
    : (setTimeout(callback, 0) as unknown as number);

const cancelFrame = (handle: number): void => {
  if (typeof cancelAnimationFrame === "function") {
    cancelAnimationFrame(handle);
  } else {
    clearTimeout(handle);
  }
};

/**
 * A per-terminal queue that hands xterm at most one frame's worth of text at a time.
 *
 * One frame drains as many queued slices as fit in `WRITE_SLICE_CHARS`, so a fast session is not
 * written one tiny piece per frame; whatever is left waits for the next frame. Slices arrive from
 * separate answers, and each answer is cut at the same budget, so the queue's head is always one
 * slice's worth or less.
 */
export function createOutputPump(
  write: (text: string) => void,
  schedule: (callback: () => void) => number = nextFrame,
  unschedule: (handle: number) => void = cancelFrame
): OutputPump {
  let queue: string[] = [];
  let handle: number | null = null;

  const pump = () => {
    handle = null;
    if (queue.length === 0) {
      return;
    }
    let out = "";
    while (queue.length > 0) {
      const head = queue[0]!;
      if (out.length > 0 && out.length + head.length > WRITE_SLICE_CHARS) {
        break;
      }
      out += head;
      queue.shift();
    }
    write(out);
    if (queue.length > 0) {
      handle = schedule(pump);
    }
  };

  return {
    push(text: string) {
      if (!text) {
        return;
      }
      for (const slice of splitWriteSlices(text)) {
        queue.push(slice);
      }
      if (handle === null) {
        handle = schedule(pump);
      }
    },
    cancel() {
      if (handle !== null) {
        unschedule(handle);
      }
      handle = null;
      queue = [];
    }
  };
}
