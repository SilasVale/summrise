// CONSOLE KEYS — the keystrokes a browser terminal cannot send, as buttons.
//
// WHY THIS EXISTS. Driving a serial console (an ONU, a switch, a bootloader) needs three
// things a keyboard in a browser cannot produce reliably:
//
//   * **Ctrl+C**: the browser owns it (copy). On a device whose shell has wedged, or whose
//     foreground command must be interrupted, there was previously NO way to send it from the
//     panel at all.
//   * **Ctrl+D / Ctrl+Z / Esc / Tab**: same shape — bound to the browser or the window.
//   * **A LINE BREAK**: not a character. It is an electrical condition on the wire, and it is
//     how an operator interrupts a bootloader's autoboot or drops into a ROM monitor. The
//     device has been able to do it since `serialport` exposed `set_break`; the operator's own
//     surface could not ask for it until now.
//
// THE BYTES ARE THE SPEC. A key here is defined by the exact bytes it sends (`bytes`) or by a
// break (`breakMs`), never by a shell escape or a name a device might not have. `bytesToBase64`
// feeds `terminal_write`'s `data_base64`, which the agent documents as "decoded and written
// exactly as given" — this file is where "exactly" is decided.
//
// WHICH KEYS A SESSION GETS depends on what it HAS: a PTY, an SSH channel and a serial line all
// have an interrupt (^C), but only a serial line has a break signal, so the BREAK button is
// offered only there. The device refuses one anywhere else by name (`TermBackend::send_break`),
// so this is a UI courtesy rather than a second rule — a stale session kind cannot make the
// button lie.

export interface ConsoleKey {
  id: string;
  /** Short enough for a dense bar. */
  label: string;
  /** What it does, in the operator's terms (the hover). */
  title: string;
  /** Exact bytes to send. Absent for a signal that is not a byte (see `breakMs`). */
  bytes?: number[];
  /** Assert a line BREAK for this many milliseconds instead of writing bytes. */
  breakMs?: number;
  /** Only offered on sessions whose kind is `serial`. */
  serialOnly?: boolean;
}

/** The bar, in the order an operator reaches for them: the interrupt first, then the
 *  end-of-input pair, then the escape hatch, then the serial-only signal last. */
export const CONSOLE_KEYS: ConsoleKey[] = [
  {
    id: "ctrl-c",
    label: "^C",
    title: "Ctrl+C — interrupt the foreground command (the browser cannot send this)",
    bytes: [0x03],
  },
  {
    id: "enter",
    label: "⏎",
    title: "Enter (carriage return)",
    bytes: [0x0d],
  },
  {
    id: "ctrl-d",
    label: "^D",
    title: "Ctrl+D — end of input / log out",
    bytes: [0x04],
  },
  {
    id: "ctrl-z",
    label: "^Z",
    title: "Ctrl+Z — suspend the foreground job",
    bytes: [0x1a],
  },
  { id: "tab", label: "Tab", title: "Tab — completion", bytes: [0x09] },
  { id: "esc", label: "Esc", title: "Escape", bytes: [0x1b] },
  {
    id: "break",
    label: "BRK",
    title:
      "Serial BREAK (250 ms) — interrupts a bootloader's autoboot, or drops into a ROM monitor. Not a character: a condition on the wire.",
    breakMs: 250,
    serialOnly: true,
  },
];

/** The keys that make sense for a session of this kind. An UNKNOWN kind (an older agent, or a
 *  kind added later) gets everything except the serial-only ones — the safe direction, because
 *  the device still refuses a break on a session that has no line. */
export function keysForKind(kind: string | null | undefined): ConsoleKey[] {
  const serial = (kind ?? "").toLowerCase() === "serial";
  return CONSOLE_KEYS.filter((k) => !k.serialOnly || serial);
}

/** base64 of exactly these bytes — the wire form `terminal_write`'s `data_base64` takes.
 *
 *  `String.fromCharCode(...bytes)` is safe here because every key above is a single control
 *  byte (or a two-byte escape sequence at most), but the function is written for a list: it
 *  encodes each byte as one character, which is what `btoa` then reads. */
export function bytesToBase64(bytes: number[]): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b & 0xff);
  return btoa(s);
}

/** The `terminal_write` params for one key — the ONE place a button's meaning becomes wire
 *  arguments, so the bar and any future caller cannot disagree about which field carries a
 *  break. */
export function keyParams(
  key: ConsoleKey,
  sessionId: string,
): { session_id: string; data_base64?: string; break_ms?: number } {
  if (typeof key.breakMs === "number") {
    return { session_id: sessionId, break_ms: key.breakMs };
  }
  return { session_id: sessionId, data_base64: bytesToBase64(key.bytes ?? []) };
}
