// Pure adopt-paging decision for TerminalPane (round-246, HIGH-3).
//
// A single terminal_read is capped at 1 MiB server-side (read_spill), so a
// chatty AI session's head used to be silently dropped when a human opened
// its tab late. The server response carries absolute cursors:
//   start  — offset the read actually began at
//   end    — offset just past the last byte returned
//   evicted— the server reset the buffer (cursor must go to 0)
// This helper decides whether ANOTHER read is needed to catch up. Pure and
// timer-free: it only inspects one response.
//
// ── RUST SINCE 2026-09-30 (block ②) ──────────────────────────────────────────────────────────────
//
// All three functions are `agent/resources/panel-logic/src/adopt.rs` now, and the differential is
// **3,076 corpus cases with 0 divergences** — values, KEY ORDER and whether each side raised — with
// every arm reached. `lib/__tests__/terminalAdopt.test.ts` runs UNCHANGED.
//
// THE TWO NUMBERS BELOW STAY, and they are passed IN: the wedged-server bound and the per-frame budget
// are this surface's own, `TerminalPane` reads both, and the test imports them — the arrangement
// `liveness.rs` records for `WORKING_MS`. `WRITE_SLICE_CHARS` is also the DEFAULT the crate cannot
// know: a JavaScript default parameter fires only for `undefined`, so the wrapper below is where that
// case is decided (`size === undefined ? WRITE_SLICE_CHARS : size` — NOT `??`, which would also
// replace a `null` the caller meant to pass).
//
// AND THE SLICES ARE CUT BY THE ENGINE, because `slice` counts UTF-16 UNITS: a slice can end between
// the halves of a surrogate pair and IS then a lone surrogate, which a Rust `String` cannot hold.
import { logic } from "../wasm/panelLogic";

export const MAX_ADOPT_PAGES = 64;

interface AdoptResponse {
  start?: number | string;
  end?: number | string;
  evicted?: boolean;
  text?: string;
  raw?: string;
}

/** True when the response proves more history exists past what we rendered. */
export function adoptNeedsAnotherPage(
  resp: AdoptResponse | null | undefined,
  rendered: number,
  advanced: boolean,
): boolean {
  return logic().adopt_needs_another_page(resp, rendered, advanced) as boolean;
}

/** Page bound: never chain more than this many reads (wedged-server guard). */
export function adoptPageExceeded(page: number): boolean {
  return logic().adopt_page_exceeded(page, MAX_ADOPT_PAGES) as boolean;
}

// P1-4 (terminal backpressure): one term.write of a full 1 MiB adopt page (or
// a burst of SSE frames) blocks the main thread while xterm parses + lays out
// the text. Slice payloads into per-frame budgets — TerminalPane queues the
// slices and writes (at most) one budget per animation frame, so a chatty
// session can never freeze the UI in a single call.

/** Max characters handed to xterm in a single animation frame. */
export const WRITE_SLICE_CHARS = 64 * 1024;

/** Split text into per-frame write slices (pure, unit-tested). */
export function splitWriteSlices(text: string, size: number = WRITE_SLICE_CHARS): string[] {
  return logic().split_write_slices(
    text,
    size === undefined ? WRITE_SLICE_CHARS : size,
  ) as string[];
}
