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
import { useEffect, useState } from "react";
import { callApi, deviceRefused } from "../lib/api";
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

const KINDS: BootKind[] = [
  "first-run",
  "clean-exit",
  "replaced",
  "machine-restart",
  "crashed",
];

const str = (v: unknown): string | null =>
  typeof v === "string" && v ? v : null;
const num = (v: unknown): number | null =>
  typeof v === "number" && Number.isFinite(v) ? v : null;

/** Read `/api/boots`'s body. Never throws; a body it cannot use is an empty history, which
 *  the card renders as "nothing to show" rather than as a device that never restarted —
 *  the failure flag is the caller's to keep (see the hook's `failed`). */
export function parseBootHistory(j: unknown): BootHistory {
  const body = (j ?? {}) as { boots?: unknown; summary?: unknown };
  const boots: BootRecord[] = (
    Array.isArray(body.boots) ? body.boots : []
  ).flatMap((raw) => {
    const r = (raw ?? {}) as Record<string, unknown>;
    const tsMs = num(r.ts_ms);
    // A record with no usable stamp is DROPPED: it cannot be placed on a time axis, and a
    // list sorted by guesswork is worse than a shorter list (the same rule the device's
    // own timeline states for its records).
    if (tsMs === null) return [];
    const rawKind = str(r.kind);
    return [
      {
        tsMs,
        kind: KINDS.find((k) => k === rawKind) ?? null,
        detail: str(r.detail) ?? "",
        uptimeSecs: num(r.uptime_secs),
        gapSecs: num(r.gap_secs),
        release: str(r.release),
      },
    ];
  });
  const s = (body.summary ?? {}) as Record<string, unknown>;
  return {
    boots,
    summary: {
      windowSecs: num(s.window_secs) ?? 86_400,
      boots: num(s.boots) ?? boots.length,
      crashes: num(s.crashes) ?? 0,
    },
  };
}

/** The history, refreshed once a minute. `failed` distinguishes "the device did not
 *  answer" from "the device has no history" — two facts this panel never collapses. */
export function useBootHistory(
  intervalMs = 60_000,
): BootHistory & { failed: boolean } {
  const [history, setHistory] = useState<BootHistory>(EMPTY_BOOT_HISTORY);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const j = await callApi("/api/boots");
        if (!alive) return;
        if (deviceRefused(j)) {
          setFailed(true);
          return;
        }
        setHistory(parseBootHistory(j));
        setFailed(false);
      } catch {
        // Keep the last good history: a missed poll is not a device that stopped
        // restarting, and blanking the list would be this panel asserting it.
        if (alive) setFailed(true);
      }
    };
    void tick();
    const t = window.setInterval(tick, intervalMs);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, [intervalMs]);
  return { ...history, failed };
}
