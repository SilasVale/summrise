import type { HostFacts } from "./api";

/**
 * The device card's browser-local vitals history.
 *
 * The console already receives `host` facts with every device it polls. What it does not have is
 * what those facts were an hour ago, and that is the question an operator asks next. This module
 * turns the poll into a bounded history: one sample per device per 30 s, at most two hours and 240
 * samples, kept in `localStorage` under one key per device so it survives a reload.
 *
 * The history is honest about its coverage. It exists only while a console page is open in this
 * browser — there is no server-side sampler and no backfill — and a sample requires an online device
 * with a positive memory total and at least one disk with a positive total. An offline or silent
 * device adds nothing, so the line never becomes a flat read of stale facts.
 *
 * Every storage access is guarded. A corrupt or foreign value loads as empty; a refused read or a
 * failed write loses history, never the page.
 */

export type VitalsSample = {
  ts_ms: number;
  memory_available_bytes: number;
  memory_total_bytes: number;
  disk_name: string;
  disk_free_bytes: number;
  disk_total_bytes: number;
};

/** The part of `Device` this module needs: the fleet entry, not the card. */
export type DeviceRef = { id: string; online: boolean; host?: HostFacts | null };

/** Keep two hours: longer than "an hour ago" and short enough to stay small. */
export const VITALS_TTL_MS = 2 * 60 * 60 * 1000;
/** 240 samples at one per 30 s is exactly the two-hour window; the cap is the backstop. */
export const VITALS_CAP = 240;
/** The App polls every 5 s; this is the cadence the stored timestamp enforces. */
export const VITALS_MIN_INTERVAL_MS = 30_000;
/** One key per device, so one device's history is never read for another. */
export const VITALS_KEY_PREFIX = "summrise.vitals.";

const NUMERIC_FIELDS = [
  "ts_ms",
  "memory_available_bytes",
  "memory_total_bytes",
  "disk_free_bytes",
  "disk_total_bytes"
] as const;

/**
 * A stored sample, or nothing.
 *
 * The stored value is untrusted: another page in this origin, an older build, or a person with the
 * devtools open can have written it. Only a full object of finite, non-negative numbers and a string
 * disk name is a sample; anything else makes the whole value read as empty rather than half-drawn.
 */
function isVitalsSample(value: unknown): value is VitalsSample {
  if (typeof value !== "object" || value === null) return false;
  const sample = value as Record<string, unknown>;
  if (typeof sample.disk_name !== "string") return false;
  return NUMERIC_FIELDS.every((field) => {
    const number = sample[field];
    return typeof number === "number" && Number.isFinite(number) && number >= 0;
  });
}

function keyFor(id: string): string {
  return VITALS_KEY_PREFIX + id;
}

/** The storage, or null when the browser refuses the property itself (site data blocked). */
function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/**
 * One device's history, newest last.
 *
 * A missing key, a corrupt JSON value, a foreign shape or a refused read all answer an empty list:
 * the chart is additive and a bad stored value must not blank the card. A well-shaped but oversized
 * array is still bounded to the newest `VITALS_CAP` entries; only `recordVitals` can know the time
 * window, but the cap needs no clock.
 */
export function loadVitals(id: string): VitalsSample[] {
  const storage = browserStorage();
  if (!storage) return [];
  let raw: string | null;
  try {
    raw = storage.getItem(keyFor(id));
  } catch {
    return [];
  }
  if (raw === null) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) && parsed.every(isVitalsSample)
      ? parsed.slice(-VITALS_CAP)
      : [];
  } catch {
    return [];
  }
}

/**
 * Sample the fleet's host facts, at most once per device per 30 s, and return the whole history by
 * device id.
 *
 * `now` is injected so the sampler has no clock of its own: the App passes `Date.now()` after a
 * successful device refresh, and tests pass their own time. A device that cannot be sampled still
 * returns the history it already had — going offline is not a reason to erase the last two hours.
 */
export function recordVitals(
  devices: readonly DeviceRef[],
  now: number
): Record<string, VitalsSample[]> {
  if (devices.length > 0) pruneRemoved(devices);
  const history: Record<string, VitalsSample[]> = {};
  for (const device of devices) {
    const samples = retain(loadVitals(device.id), now);
    const latest = samples[samples.length - 1];
    const due = latest === undefined || now - latest.ts_ms >= VITALS_MIN_INTERVAL_MS;
    const fresh = device.online && due ? sampleFor(device, now) : null;
    if (!fresh) {
      history[device.id] = samples;
      continue;
    }
    samples.push(fresh);
    const bounded = samples.slice(-VITALS_CAP);
    writeVitals(device.id, bounded);
    history[device.id] = bounded;
  }
  return history;
}

/**
 * Forget keys for devices the fleet no longer lists, so a removed machine does not leave a history
 * in this browser forever. An empty list is not a fleet: a partial or failed refresh must not erase
 * the histories of devices that are still there.
 */
function pruneRemoved(devices: readonly DeviceRef[]): void {
  const storage = browserStorage();
  if (!storage) return;
  const present = new Set(devices.map((device) => keyFor(device.id)));
  try {
    const stale: string[] = [];
    for (let index = 0; index < storage.length; index += 1) {
      const key = storage.key(index);
      if (key !== null && key.startsWith(VITALS_KEY_PREFIX) && !present.has(key)) {
        stale.push(key);
      }
    }
    for (const key of stale) storage.removeItem(key);
  } catch {
    /* a refused enumeration or removal leaves the keys; the next successful call retries */
  }
}

/** Two hours, and never more than the cap, keep the stored value and the returned chart bounded. */
function retain(samples: VitalsSample[], now: number): VitalsSample[] {
  return samples.filter((sample) => now - sample.ts_ms <= VITALS_TTL_MS).slice(-VITALS_CAP);
}

/**
 * The facts to store, or null when they cannot make an honest sample.
 *
 * A zero memory total makes a percentage meaningless; a missing available figure leaves nothing to
 * draw. The disk is the tightest one: the lowest free percentage, not the lowest absolute free
 * space — a huge disk with a few percent left is the one that fills first. A zero total is skipped
 * rather than divided by.
 */
function sampleFor(device: DeviceRef, now: number): VitalsSample | null {
  const memory = device.host?.memory;
  const total = memory?.total_bytes;
  const available = memory?.available_bytes;
  if (typeof total !== "number" || !Number.isFinite(total) || total <= 0) return null;
  if (typeof available !== "number" || !Number.isFinite(available) || available < 0) return null;
  const disk = tightestDisk(device.host?.disks);
  if (!disk) return null;
  return {
    ts_ms: now,
    memory_available_bytes: available,
    memory_total_bytes: total,
    disk_name: disk.name,
    disk_free_bytes: disk.free_bytes,
    disk_total_bytes: disk.total_bytes
  };
}

type Disk = { name: string; total_bytes: number; free_bytes: number };

/** The disk with the lowest free percentage among those with a positive total. */
function tightestDisk(disks: HostFacts["disks"]): Disk | null {
  let tightest: Disk | null = null;
  let tightestRatio = Number.POSITIVE_INFINITY;
  for (const disk of disks ?? []) {
    if (!disk || typeof disk.name !== "string") continue;
    const { total_bytes: total, free_bytes: free } = disk;
    if (typeof total !== "number" || !Number.isFinite(total) || total <= 0) continue;
    if (typeof free !== "number" || !Number.isFinite(free) || free < 0) continue;
    const ratio = free / total;
    if (ratio < tightestRatio) {
      tightestRatio = ratio;
      tightest = disk;
    }
  }
  return tightest;
}

/** A failed write (quota, private mode, a blocking extension) loses history, never the poll. */
function writeVitals(id: string, samples: VitalsSample[]): void {
  const storage = browserStorage();
  if (!storage) return;
  try {
    storage.setItem(keyFor(id), JSON.stringify(samples));
  } catch {
    /* the page keeps the samples it just sampled; the next write may succeed */
  }
}
