import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HostFacts } from "./api";
import {
  loadVitals,
  recordVitals,
  VITALS_CAP,
  VITALS_KEY_PREFIX,
  VITALS_MIN_INTERVAL_MS,
  VITALS_TTL_MS,
  type DeviceRef,
  type VitalsSample
} from "./vitals";

const GIB = 1024 ** 3;
const NOW = 1_700_000_000_000;

function host(overrides: Partial<HostFacts> = {}): HostFacts {
  return {
    memory: { total_bytes: 8 * GIB, available_bytes: 4 * GIB },
    disks: [{ name: "/", total_bytes: 100 * GIB, free_bytes: 20 * GIB }],
    ...overrides
  };
}

function device(overrides: Partial<DeviceRef> = {}): DeviceRef {
  return { id: "dev-1", online: true, host: host(), ...overrides };
}

function sample(ts_ms: number, overrides: Partial<VitalsSample> = {}): VitalsSample {
  return {
    ts_ms,
    memory_available_bytes: 4 * GIB,
    memory_total_bytes: 8 * GIB,
    disk_name: "/",
    disk_free_bytes: 20 * GIB,
    disk_total_bytes: 100 * GIB,
    ...overrides
  };
}

function stored(id = "dev-1"): VitalsSample[] {
  const raw = localStorage.getItem(VITALS_KEY_PREFIX + id);
  return raw === null ? [] : (JSON.parse(raw) as VitalsSample[]);
}

beforeEach(() => {
  localStorage.clear();
});

describe("recordVitals", () => {
  it("takes at most one sample per device per 30 s and persists it under that device's key", () => {
    const first = recordVitals([device()], NOW);
    expect(first["dev-1"]).toHaveLength(1);
    expect(first["dev-1"][0]).toMatchObject({
      ts_ms: NOW,
      memory_available_bytes: 4 * GIB,
      memory_total_bytes: 8 * GIB,
      disk_name: "/",
      disk_free_bytes: 20 * GIB,
      disk_total_bytes: 100 * GIB
    });
    expect(stored()).toEqual(first["dev-1"]);

    // The App polls every 5 s; the stored timestamp is what keeps the history at one per 30 s.
    expect(recordVitals([device()], NOW + VITALS_MIN_INTERVAL_MS - 1)["dev-1"]).toHaveLength(1);
    const due = recordVitals([device()], NOW + VITALS_MIN_INTERVAL_MS)["dev-1"];
    expect(due.map((entry) => entry.ts_ms)).toEqual([NOW, NOW + VITALS_MIN_INTERVAL_MS]);
    expect(stored()).toEqual(due);
  });

  it("drops samples older than two hours when it records", () => {
    const stale = sample(NOW - VITALS_TTL_MS - 1);
    const recent = sample(NOW - 90 * 60_000);
    localStorage.setItem(VITALS_KEY_PREFIX + "dev-1", JSON.stringify([stale, recent]));

    const history = recordVitals([device()], NOW)["dev-1"];
    expect(history.map((entry) => entry.ts_ms)).toEqual([recent.ts_ms, NOW]);
    expect(loadVitals("dev-1")).toEqual(history);
  });

  it("caps the history at 240 samples, dropping the oldest", () => {
    // 29 s spacing keeps every seeded sample inside the two-hour window, so this test is about the
    // cap and not about retention.
    const gap = VITALS_MIN_INTERVAL_MS - 1000;
    const seeded = Array.from({ length: VITALS_CAP }, (_, index) =>
      sample(NOW - (VITALS_CAP - 1 - index) * gap)
    );
    localStorage.setItem(VITALS_KEY_PREFIX + "dev-1", JSON.stringify(seeded));

    const now = NOW + VITALS_MIN_INTERVAL_MS;
    const history = recordVitals([device()], now)["dev-1"];
    expect(history).toHaveLength(VITALS_CAP);
    expect(history[0].ts_ms).toBe(seeded[1].ts_ms);
    expect(history[VITALS_CAP - 1].ts_ms).toBe(now);
  });

  it("adds nothing for offline devices or facts that cannot make an honest sample", () => {
    const devices: DeviceRef[] = [
      device({ id: "offline", online: false }),
      device({ id: "no-host", host: null }),
      device({ id: "no-memory", host: host({ memory: undefined }) }),
      device({ id: "zero-memory", host: host({ memory: { total_bytes: 0, available_bytes: 0 } }) }),
      device({ id: "no-available", host: host({ memory: { total_bytes: 8 * GIB } }) }),
      device({ id: "no-disks", host: host({ disks: [] }) }),
      device({ id: "zero-disk", host: host({ disks: [{ name: "/", total_bytes: 0, free_bytes: 0 }] }) })
    ];

    const history = recordVitals(devices, NOW);
    expect(Object.keys(history).sort()).toEqual(devices.map((entry) => entry.id).sort());
    for (const id of devices.map((entry) => entry.id)) {
      expect(history[id]).toEqual([]);
      expect(localStorage.getItem(VITALS_KEY_PREFIX + id)).toBeNull();
    }
  });

  it("keeps the history it already has when the device goes offline", () => {
    recordVitals([device()], NOW);

    const later = recordVitals([device({ online: false })], NOW + 2 * VITALS_MIN_INTERVAL_MS);
    expect(later["dev-1"].map((entry) => entry.ts_ms)).toEqual([NOW]);
  });

  it("draws the tightest disk: the lowest free percentage, skipping zero totals", () => {
    const facts = host({
      disks: [
        // Bigger absolute free space, but the lower percentage — the one that fills first.
        { name: "archive", total_bytes: 1000 * GIB, free_bytes: 50 * GIB },
        { name: "root", total_bytes: 100 * GIB, free_bytes: 10 * GIB },
        { name: "unmounted", total_bytes: 0, free_bytes: 0 }
      ]
    });

    const [only] = recordVitals([device({ host: facts })], NOW)["dev-1"];
    expect(only).toMatchObject({
      disk_name: "archive",
      disk_free_bytes: 50 * GIB,
      disk_total_bytes: 1000 * GIB
    });
  });

  it("keeps each device's history under its own key", () => {
    const other = sample(NOW, { memory_available_bytes: 1 * GIB, disk_name: "other" });
    localStorage.setItem(VITALS_KEY_PREFIX + "dev-2", JSON.stringify([other]));

    // dev-2 is in the fleet (offline, no host), so its key is kept but never read for dev-1.
    const history = recordVitals([device(), { id: "dev-2", online: false }], NOW);
    expect(history["dev-1"].map((entry) => entry.ts_ms)).toEqual([NOW]);
    expect(history["dev-1"][0].memory_available_bytes).toBe(4 * GIB);
    expect(loadVitals("dev-2")).toEqual([other]);
  });

  it("forgets a removed device's history and keeps a listed device's", () => {
    localStorage.setItem(VITALS_KEY_PREFIX + "gone", JSON.stringify([sample(NOW)]));
    localStorage.setItem(VITALS_KEY_PREFIX + "dev-1", JSON.stringify([sample(NOW - 30_000)]));

    recordVitals([device()], NOW);

    expect(localStorage.getItem(VITALS_KEY_PREFIX + "gone")).toBeNull();
    expect(loadVitals("dev-1")).toHaveLength(2);
  });

  it("loads corrupt or foreign storage as empty and replaces it on the next sample", () => {
    const key = VITALS_KEY_PREFIX + "dev-1";
    const corrupt = [
      "{not json",
      "null",
      JSON.stringify({ samples: [sample(NOW)] }),
      JSON.stringify([sample(NOW), { ...sample(NOW), memory_total_bytes: "8" }]),
      JSON.stringify([sample(NOW, { memory_available_bytes: -1 })])
    ];

    for (const raw of corrupt) {
      localStorage.setItem(key, raw);
      expect(loadVitals("dev-1")).toEqual([]);
    }

    const history = recordVitals([device()], NOW)["dev-1"];
    expect(history).toHaveLength(1);
    expect(stored()).toEqual(history);
  });

  it("never returns more than the cap, even from a foreign but well-shaped array", () => {
    const oversized = Array.from({ length: VITALS_CAP + 10 }, (_, index) => sample(NOW + index));
    localStorage.setItem(VITALS_KEY_PREFIX + "dev-1", JSON.stringify(oversized));

    const loaded = loadVitals("dev-1");
    expect(loaded).toHaveLength(VITALS_CAP);
    expect(loaded).toEqual(oversized.slice(-VITALS_CAP));
  });

  it("survives storage that refuses to be read", () => {
    const getItem = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new DOMException("denied", "SecurityError");
    });

    expect(loadVitals("dev-1")).toEqual([]);
    expect(() => recordVitals([device()], NOW)).not.toThrow();
    expect(getItem).toHaveBeenCalled();
  });

  it("survives a storage write that fails, such as quota or private mode", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("full", "QuotaExceededError");
    });

    const history = recordVitals([device()], NOW)["dev-1"];
    expect(history).toHaveLength(1);
    expect(history[0].ts_ms).toBe(NOW);
  });
});
