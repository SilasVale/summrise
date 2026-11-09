import type { HostFacts } from "./api";
import { spanText } from "./age";
import { clockTime, exactMoment } from "./format";

/**
 * THE RECOVERY LINE: WAS THIS MACHINE AWAY, AND FOR HOW LONG, BEFORE IT CAME BACK?
 *
 * A machine that was gone for a day comes back `online` with a fresh `started 27m ago`; the outage
 * is nowhere on the page (audit-44 F12). This module builds the one muted sentence beside the start
 * that says how long it was gone, from the two records the console actually has:
 *
 *   back after 14 h 57 m (the device's journal recorded its boot at 08:51)
 *   back after 41 m
 *   The console restarted while this machine was away, so it cannot vouch for how long it was gone.
 *
 * **TWO SOURCES, ONE RULE: THE GAP MUST BE PROVEN.** The console's own channel pair
 * (`last_outage`: a `device.left` and the `device.joined` that closed it) is the exact gap between
 * the last frame before the silence and the first after, so it is preferred and stated without
 * qualification. When that pair was never written — the measured CPH vanished without the console
 * writing a close — the device's own boot journal is the fallback: the entry at the machine's host
 * boot (the frame's `uptime_s` proves which one) is the return, and the entry before it is the last
 * boot the journal knows. The line then says the gap is the journal's, never that the console saw a
 * frame there. A machine with neither record draws no line: the page does not estimate a gap.
 *
 * **A CONSOLE THAT RESTARTED IN THE GAP CANNOT VOUCH.** The console's start time (`/health`'s
 * uptime) inside either gap means part of that silence was the console's own downtime, not the
 * machine's absence; the duration is withheld and the sentence says why. Without that check a
 * console restart would be charged to the machine.
 *
 * **FIVE MINUTES IS THE FLOOR.** A channel blip is not a recovery: an apply that restarts the app
 * closes and reopens the channel in seconds, and `back after 2 s` beside the start is noise, not
 * news. This is the same floor the fleet row used when it first drew this sentence (8e37d89).
 */

/** A recovery shorter than this is a blip, not news. */
export const RECOVERY_FLOOR_MS = 5 * 60_000;

/**
 * How far the device's app start may sit from the host's boot and still be that boot's entry.
 *
 * The launcher starts the app after the host boots; the measured CPH started it four seconds later.
 * Two minutes covers a slow start while still refusing to read a later app restart as the boot.
 */
const HOST_BOOT_MATCH_MS = 2 * 60_000;

/** A journal entry before the host boot cannot be its app start; a few seconds of clock skew is not. */
const CLOCK_SLACK_MS = 5_000;

/** What a recovery line needs from the fleet row: the console's pair, the frame, and the journal. */
export type RecoveryDevice = {
  last_outage?: { away_ms: number; back_at_ms: number } | null;
  last_frame_ms?: number | null;
  host?: Pick<HostFacts, "restarts" | "uptime_s"> | null;
};

export type RecoveryStory = {
  text: string;
  /** The exact return instant for the line's title; absent when the return itself is unproven. */
  title: string | null;
  /** The record the gap came from; the journal form names it in the sentence. */
  source: "console" | "journal";
};

/** The sentence a console that restarted inside the gap draws instead of a duration. */
export const CONSOLE_RESTART_SENTENCE =
  "The console restarted while this machine was away, so it cannot vouch for how long it was gone.";

/**
 * The recovery sentence for one machine, or `null` when no gap is proven.
 *
 * `consoleStartedAt` is `/health`'s uptime turned into the moment this console process began; absent
 * when that read failed, in which case the gap is stated without the restart check rather than
 * guessed at.
 */
export function recoveryStory(
  device: RecoveryDevice | null | undefined,
  consoleStartedAt?: number | null
): RecoveryStory | null {
  if (!device) return null;
  const outage = device.last_outage;
  if (
    outage &&
    Number.isFinite(outage.away_ms) &&
    Number.isFinite(outage.back_at_ms) &&
    outage.away_ms > 0
  ) {
    const beganAt = outage.back_at_ms - outage.away_ms;
    if (consoleRestartedIn(beganAt, outage.back_at_ms, consoleStartedAt)) {
      return { text: CONSOLE_RESTART_SENTENCE, title: exactMoment(outage.back_at_ms), source: "console" };
    }
    if (outage.away_ms < RECOVERY_FLOOR_MS) return null;
    return {
      text: `back after ${spanText(outage.away_ms)}`,
      title: exactMoment(outage.back_at_ms),
      source: "console"
    };
  }
  return journalRecovery(device, consoleStartedAt);
}

/**
 * The fallback: a gap between the journal's boot at the machine's own start and the boot before it.
 *
 * The frame's `uptime_s` says when the host booted; the journal entry at that moment is the app the
 * machine started on return. The entry before it is the newest boot the device recorded before it
 * went away, so the interval between the two is the gap the device itself proves — stated as the
 * journal's fact, never as a frame the console watched. No matching entry, no earlier entry, or a
 * gap under the floor draws nothing.
 */
function journalRecovery(
  device: RecoveryDevice,
  consoleStartedAt?: number | null
): RecoveryStory | null {
  const restarts = device.host?.restarts;
  const uptimeSeconds = device.host?.uptime_s;
  if (!Array.isArray(restarts) || restarts.length < 2) return null;
  if (typeof uptimeSeconds !== "number" || !Number.isFinite(uptimeSeconds) || uptimeSeconds < 0) {
    return null;
  }
  // The frame that carried `uptime_s` is the closest clock the page has to it; a frame's age would
  // otherwise move the host's boot by that age. `last_frame_ms` is the newest frame of any kind, so
  // it can be a little later than the host report — the match window absorbs that.
  const reference =
    typeof device.last_frame_ms === "number" && Number.isFinite(device.last_frame_ms)
      ? device.last_frame_ms
      : Date.now();
  const hostBootAt = reference - uptimeSeconds * 1000;

  // The OLDEST entry inside the match window is the first app start after the host boot; a newer
  // entry in the same window is an app restart during that start, not the boot itself.
  let bootIndex = -1;
  for (let index = 0; index < restarts.length; index += 1) {
    const at = restarts[index]?.at_ms;
    if (typeof at !== "number" || !Number.isFinite(at)) continue;
    if (at < hostBootAt - CLOCK_SLACK_MS) continue;
    if (at - hostBootAt > HOST_BOOT_MATCH_MS) continue;
    bootIndex = index;
  }
  if (bootIndex < 0 || bootIndex + 1 >= restarts.length) return null;
  const backAt = restarts[bootIndex]?.at_ms;
  const beforeAt = restarts[bootIndex + 1]?.at_ms;
  if (
    typeof backAt !== "number" ||
    typeof beforeAt !== "number" ||
    !Number.isFinite(backAt) ||
    !Number.isFinite(beforeAt)
  ) {
    return null;
  }
  const gap = backAt - beforeAt;
  if (gap <= 0) return null;
  if (consoleRestartedIn(beforeAt, backAt, consoleStartedAt)) {
    return { text: CONSOLE_RESTART_SENTENCE, title: exactMoment(backAt), source: "journal" };
  }
  if (gap < RECOVERY_FLOOR_MS) return null;
  return {
    text: `back after ${spanText(gap)} (the device's journal recorded its boot at ${clockTime(backAt)})`,
    title: exactMoment(backAt),
    source: "journal"
  };
}

/**
 * Whether this console process began inside `[beganAt, backAt]`.
 *
 * The upper end is included: a console that started exactly at the return still missed the silence,
 * and its join is the first thing it saw rather than the end of a gap it watched.
 */
function consoleRestartedIn(
  beganAt: number,
  backAt: number,
  consoleStartedAt: number | null | undefined
): boolean {
  if (typeof consoleStartedAt !== "number" || !Number.isFinite(consoleStartedAt)) return false;
  return consoleStartedAt > beganAt && consoleStartedAt <= backAt;
}
