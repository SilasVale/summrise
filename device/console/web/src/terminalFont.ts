/**
 * The terminal's text size.
 *
 * A preference of the operator's, not part of any session: it lives in `localStorage` under one key,
 * never travels over the wire, and is clamped to a range the layout survives. The storage is passed
 * in (rather than read here) so the rules are a testable function, and every read and write is
 * wrapped because a browser in private mode answers with an exception instead of a value.
 * `fontStorage` is the one place the browser's own storage is looked up: with site data blocked even
 * `window.localStorage` throws, and a page must still render.
 */
export const MIN_FONT_SIZE = 10;
export const MAX_FONT_SIZE = 20;
export const DEFAULT_FONT_SIZE = 13;
export const FONT_SIZE_KEY = "summrise.console.terminal.fontSize";

/** The browser's storage, or null when the browser refuses the property itself. */
export function fontStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** A finite, whole size inside the range; `NaN` is the default, an infinity is the nearer bound. */
export function clampFontSize(value: number): number {
  if (Number.isNaN(value)) return DEFAULT_FONT_SIZE;
  return Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, Math.round(value)));
}

/** The stored size, or the default when there is no storage, no value, or an unreadable one. */
export function loadFontSize(storage: Pick<Storage, "getItem"> | null): number {
  if (!storage) return DEFAULT_FONT_SIZE;
  let raw: string | null;
  try {
    raw = storage.getItem(FONT_SIZE_KEY);
  } catch {
    return DEFAULT_FONT_SIZE;
  }
  if (raw === null || raw.trim() === "") return DEFAULT_FONT_SIZE;
  const stored = Number(raw);
  if (Number.isNaN(stored)) return DEFAULT_FONT_SIZE;
  return clampFontSize(stored);
}

/** Remember the size. A storage that refuses the write is not a failure of the terminal. */
export function saveFontSize(storage: Pick<Storage, "setItem"> | null, size: number): void {
  if (!storage) return;
  try {
    storage.setItem(FONT_SIZE_KEY, String(clampFontSize(size)));
  } catch {
    /* private mode, a full quota: the size still applies to this visit */
  }
}
