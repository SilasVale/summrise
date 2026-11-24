/**
 * What a person typed in one `terminal.write`, moved out of the terminal component so the session's
 * activity reads the same evidence the same way.
 *
 * xterm sends the terminal's focus reports through `onData` (`ESC[I` on focus, `ESC[O` on blur), and
 * cursor and function keys are escape sequences too. The session receives them — that is the
 * terminal's job — but they are not text a person typed, so neither the last-line card nor the
 * activity rail may present one as input. A space is a keystroke (the space bar) and survives; a
 * write whose remainder is only control characters (Enter, Tab, Backspace, a stray BEL) has nothing
 * to show.
 */

/**
 * One ANSI escape sequence, as much of the grammar a terminal sends as input: CSI
 * (`ESC [ parameters final`, where the focus reports `ESC[I`/`ESC[O` live), SS3 (`ESC O final`),
 * OSC and the other string controls (closed by BEL or ST), and any other escape.
 *
 * Matched whole on purpose: a sequence's final byte is printable (`I`, `O`, `A` …), so a test for
 * "does this write contain a printable character" must remove the sequence first or the sequence's
 * own letter reads as a typed key.
 */
const ANSI_ESCAPE =
  /\x1b\[[0-?]*[ -/]*[@-~]|\x1bO[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)?|\x1b[PX^_][^\x1b]*(?:\x1b\\)?|\x1b[ -/]*[@-~]?/g;

/**
 * What a person typed in one `terminal.write`, or null when the write carried no keystroke.
 *
 * Kept in this module rather than beside either reader: the terminal's last-line card and the
 * session's activity must agree on what counts as typing, and two copies would drift.
 */
export function typedText(input: string): string | null {
  const text = input.replace(ANSI_ESCAPE, "");
  return /[^\x00-\x1f\x7f-\x9f]/.test(text) ? text : null;
}

/**
 * How far apart two writes may be and still belong to one typed line.
 *
 * **ONE KEYSTROKE IS NOT ONE WRITE.** xterm sends each keystroke through `onData`; the write batcher
 * sends the first chunk immediately and holds what arrives while that call is on the wire, so a line
 * typed faster than the console's round trip leaves as two or more writes. Measured 2026-11-06 on
 * the 84T: `echo audit37-terminal` reached the audit as two writes 923 ms apart, and the card drew
 * only the second, `cho audit37-terminal` — the first character was the previous write.
 *
 * Two seconds covers a slow round trip and is still shorter than the pause before a person starts
 * the next command. The evidence carries no control bytes (the console's excerpt flattens and trims
 * them), so an Enter and a Backspace are both an `input`-less write and cannot be told apart here.
 * Both now end the burst: an Enter must not let the command typed after it join the one before it.
 * The price is that a Backspace can start a fragment — the write after it is not joined to the line
 * it edited — and that is accepted, because the other error draws one card for two commands.
 */
export const WRITE_BURST_MS = 2_000;
