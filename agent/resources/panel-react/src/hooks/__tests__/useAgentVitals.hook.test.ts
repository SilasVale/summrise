// THE HOOK'S SIDE OF THE CONTRACT, which the boot-verdict tests next door cannot reach: those pin
// `parseLastBoot` as a pure function, so what the HOOK does with a response was untested.
//
// The property (2026-09-24, the panel exploration): a FAILED read must not update the strip. Every
// sibling poller gated on the refusal predicate — `useMonitors`, `useVitalsSeries`, `useBootHistory`,
// `UpdateCard` — and this one guarded on `!j`, which an empty object passes because `{}` is truthy. A
// refusal that carried fields would therefore have been read as a sample.
//
// THE SEAM MOVED, THE PROPERTY DID NOT (the read loop is `useDeviceRead`'s now). These tests used to
// pass `useAgentVitals(10)` and wait 30 ms for "the 10 ms poll" to ride through a changed mock — a
// cadence that NO LONGER EXISTS, because a cadence is floored at 5 s. Left as it was, the two failure
// tests would still have passed while proving nothing: no poll would have happened at all inside the
// window. So each one now DRIVES THE TIMER to the floor and asserts the extra call happened first,
// which is what makes the assertion about the poll rather than about the absence of one.
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { callApi } from "../../lib/api";
import { useAgentVitals } from "../useAgentVitals";

// SPREAD THE REAL MODULE, MOCK ONLY THE TRANSPORT. This said `() => ({ callApi: vi.fn() })`, which
// replaces EVERY export of `lib/api` — so when the hook started asking `deviceRefused(j)` (the shared
// predicate behind "a failed read must not update"), the mock answered `undefined`, every poll threw
// into the hook's own catch, and the strip simply never updated. All three tests here failed with an
// empty value, INCLUDING the two whose fixtures carry `ok: true`, because the throw happens before any
// fixture is read. `tsc` cannot see it: a `vi.mock` factory is not checked against the module.
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  callApi: vi.fn(),
}));

const mockCallApi = callApi as unknown as ReturnType<typeof vi.fn>;

/** The cadence this hook runs at when a caller asks for one below the module's floor: a read loop
 *  floors `everyMs` at 5 s (`useDeviceRead`), so a poll is 5 s away whatever a test passes. */
const FLOOR_MS = 5_000;

/** Flush a settled read and let React commit it. */
const flush = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });

/** Drive the clock to the floored cadence: the next read happens HERE and nowhere sooner. */
const poll = () =>
  act(async () => {
    await vi.advanceTimersByTimeAsync(FLOOR_MS);
  });

beforeEach(() => {
  vi.useFakeTimers();
  mockCallApi.mockReset();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("useAgentVitals", () => {
  it("takes a good sample", async () => {
    mockCallApi.mockResolvedValue({
      ok: true,
      uptime_secs: 3661,
      host: "10.0.0.5",
      port: 18080,
    });
    const { result } = renderHook(() => useAgentVitals(10));
    await flush();
    expect(result.current.uptime).toBe("1h 01m");
    expect(result.current.host).toBe("10.0.0.5");
  });

  it("IGNORES a refusal that carries fields, keeping the last known values", async () => {
    mockCallApi.mockResolvedValue({
      ok: true,
      uptime_secs: 3661,
      host: "10.0.0.5",
    });
    const { result } = renderHook(() => useAgentVitals(10));
    await flush();
    expect(result.current.uptime).toBe("1h 01m");

    // The device refuses the next read — and the body carries fields anyway, which is the case the old
    // `!j` guard let through. Nothing on the strip may move.
    mockCallApi.mockResolvedValue({
      ok: false,
      error: "nope",
      uptime_secs: 0,
      host: "somewhere-else",
    });
    await poll();
    // THE REFUSAL WAS ACTUALLY READ, not merely absent: without this the test would pass on a hook
    // that had stopped polling altogether.
    expect(mockCallApi).toHaveBeenCalledTimes(2);
    expect(result.current.uptime).toBe("1h 01m");
    expect(result.current.host).toBe("10.0.0.5");
  });

  it("ignores an EMPTY object too, which is what the design harness answers unknown routes with", async () => {
    mockCallApi.mockResolvedValue({ ok: true, uptime_secs: 60 });
    const { result } = renderHook(() => useAgentVitals(10));
    await flush();
    expect(result.current.uptime).toBe("1m 0s");

    // `{}` carries no `ok` at all, so it is a REFUSAL to the shared predicate (the strict form:
    // the device always sends `ok: true` on success) — never a sample of nothing.
    mockCallApi.mockResolvedValue({});
    await poll();
    expect(mockCallApi).toHaveBeenCalledTimes(2);
    expect(result.current.uptime).toBe("1m 0s");
  });
});
