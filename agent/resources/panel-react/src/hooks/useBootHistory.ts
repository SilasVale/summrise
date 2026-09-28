// The device's RESTART HISTORY — how often this agent starts, and how each run before it
// ended. `/api/boots`, polled slowly.
//
// WHY IT EXISTS. `/api/status`'s `last_boot` answers "how did the run before this one end",
// and the device OVERWRITES that verdict at every boot — so the pattern, which is the thing
// an operator actually acts on, was invisible. d1's 2026-09-13 incident ("the agent
// restarted every one to two hours") could not even be COUNTED while it was happening. The
// agent now keeps a bounded history (see `runstate.rs`), and this hook is its reader.
//
// WHY A SEPARATE POLL FROM `/api/status`. The status poll runs every 15 s and must stay
// small; the history changes at most once per boot. A minute is fast enough that a device
// which restarts while the operator watches shows up, and cheap enough that a device that
// never restarts costs one small request a minute.
//
// ── THE PARSE IS RUST NOW (P2, 2026-09-28) ───────────────────────────────────
//
// `parseBootHistory` moved to `agent/resources/panel-logic/src/boot.rs` — the same crate and the
// same loader as `lib/archive.ts`'s parse, so the pipeline cost was paid once. It is `async` for
// the reason that file's header records: the wasm is fetched at the first call, never at page load
// (criterion ③ of the migration plan), so a migrated function cannot answer synchronously during
// render. This one is a `useDeviceRead` fold — already asynchronous — so the hook's own call site
// is unchanged (`reduce` may return a promise now, and the module awaits it).
import { useDeviceRead } from "./useDeviceRead";
import { panelLogic } from "../wasm/panelLogic";
import type { BootKind } from "./useAgentVitals";

export interface BootRecord {
  /** Unix MILLISECONDS — the unit the timeline and every panel clock use. */
  tsMs: number;
  /** The verdict for the run that preceded this boot; null when this agent is newer than
   *  the vocabulary (an unrecognised spelling renders as "unrecorded", never as a guess). */
  kind: BootKind | null;
  /** The device's own sentence about that run. */
  detail: string;
  /** How long the previous run lived, when there was one to measure. */
  uptimeSecs: number | null;
  /** How long the device went without an agent before this boot. */
  gapSecs: number | null;
  /** The release this boot came UP on, when the device knows it. */
  release: string | null;
}

interface BootSummary {
  windowSecs: number;
  boots: number;
  crashes: number;
}

export interface BootHistory {
  boots: BootRecord[];
  summary: BootSummary;
}

export const EMPTY_BOOT_HISTORY: BootHistory = {
  boots: [],
  summary: { windowSecs: 86_400, boots: 0, crashes: 0 },
};

/** Read `/api/boots`'s body. Never throws; a body it cannot use is an empty history, which
 *  the card renders as "nothing to show" rather than as a device that never restarted —
 *  the failure flag is the caller's to keep (see the hook's `failed`).
 *
 *  RUST SINCE P2 (2026-09-28): the body above is `agent/resources/panel-logic/src/boot.rs`. The
 *  rules it keeps are the ones written here and they are unchanged — a record with no usable stamp
 *  is DROPPED, an unrecognised `kind` renders as "unrecorded" rather than borrowing another kind's
 *  words, and nothing throws. `KINDS`, `str` and `num` are gone from this file with it, so there is
 *  no second derivation of the restart history left in the panel.
 *
 *  `EMPTY_BOOT_HISTORY` STAYS: it is the value `useDeviceRead` starts from, not a parse result, and
 *  the test that pins `parseBootHistory(null)` against it is what keeps the two agreeing. */
export async function parseBootHistory(j: unknown): Promise<BootHistory> {
  const logic = await panelLogic();
  return logic.parse_boot_history(j) as BootHistory;
}

/** The history, refreshed once a minute. `failed` distinguishes "the device did not
 *  answer" from "the device has no history" — two facts this panel never collapses. */
export function useBootHistory(
  intervalMs = 60_000,
): BootHistory & { failed: boolean } {
  // THE READ LOOP IS `useDeviceRead`'s (see its header): the refusal guard, the cadence
  // floor this reader never had, the unmount guard and the ordering guard. Keep-last is
  // what a missed poll gets — a missed poll is not a device that stopped restarting.
  const { data, read } = useDeviceRead<BootHistory>({
    path: "/api/boots",
    // A body this build cannot use is an empty history — never a throw (parseBootHistory).
    reduce: (_previous, body) => parseBootHistory(body),
    initial: EMPTY_BOOT_HISTORY,
    everyMs: intervalMs,
  });
  return { ...data, failed: read === "unreadable" };
}
