// useOperationRuns — the poll, and the properties the strip depends on:
// it asks only for what it has not seen, it never double-counts the record that
// sits ON the cursor, a failed poll never blanks what the operator is reading,
// and the cadence is the shared floor's rather than whatever a caller passed.
//
// THE TIMERS ARE FAKE, AND THAT IS THE MIGRATION SHOWING THROUGH. The loop is `useDeviceRead`'s
// now, and its cadence floor is 5 s, so `useOperationRuns(25)` no longer polls every 25 ms — the
// old real-timer `waitFor(…, 3000)` was asserting a cadence the panel does not permit. Every
// repeat read below is therefore driven by advancing the clock past the floor, which doubles as
// the reason the cadence test at the bottom exists.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useOperationRuns } from "../useOperationRuns";
import { callApi } from "../../lib/api";
import type { OperationEvent } from "../../lib/runs";

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  callApi: vi.fn(),
}));

const mockCallApi = callApi as unknown as ReturnType<typeof vi.fn>;

const T0 = 1_700_000_000_000;
const ev = (ts_ms: number, command: string): OperationEvent => ({
  source: "terminal",
  ts_ms,
  kind: "command/start",
  command,
});

/** The `since_ms` a given call asked for. */
function sinceOf(call: unknown[]): number {
  const path = String(call[0]);
  const m = path.match(/since_ms=(\d+)/);
  return m ? Number(m[1]) : Number.NaN;
}

/** Let the mount read (whose mock is already resolved) land, and React commit it. */
const flush = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });

/** A repeat read: advance past the module's 5 s cadence floor, then let it land. The cursor is
 *  read inside the `path` FUNCTION at that moment, which is the property the cursor tests pin. */
const tick = (ms = 5_000) =>
  act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
    await Promise.resolve();
    await Promise.resolve();
  });

beforeEach(() => {
  vi.useFakeTimers();
  mockCallApi.mockReset();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("useOperationRuns", () => {
  it("polls /api/operation with a since cursor and a bounded limit", async () => {
    mockCallApi.mockResolvedValue({
      ok: true,
      events: [ev(T0, "ls")],
      runs: [],
      cursor_ms: T0,
    });
    const { result } = renderHook(() => useOperationRuns(60_000));
    await flush();
    expect(result.current.events).toHaveLength(1);
    expect(String(mockCallApi.mock.calls[0][0])).toMatch(
      /^\/api\/operation\?since_ms=0&limit=\d+$/,
    );
  });

  it("asks only for what it has not seen, and does NOT double-count the record on the cursor", async () => {
    // `since_ms` filters `ts_ms < since_ms`, so the record stamped exactly AT
    // the cursor is sent again by the device. Counting it twice would inflate
    // every number on the strip by one per poll.
    const boundary = ev(T0 + 1_000, "second");
    mockCallApi
      .mockResolvedValueOnce({
        ok: true,
        events: [ev(T0, "first"), boundary],
        runs: [],
        cursor_ms: T0 + 1_000,
      })
      .mockResolvedValue({
        ok: true,
        events: [boundary, ev(T0 + 2_000, "third")],
        runs: [],
        cursor_ms: T0 + 2_000,
      });
    const { result } = renderHook(() => useOperationRuns(5_000));
    await flush();
    await tick();
    expect(result.current.events.map((e) => e.command)).toEqual([
      "first",
      "second",
      "third",
    ]);
    // The second request picked up where the first reply ended — the cursor is resolved per
    // read, not captured when the hook was called.
    expect(mockCallApi.mock.calls.length).toBeGreaterThan(1);
    expect(sinceOf(mockCallApi.mock.calls[1])).toBe(T0 + 1_000);
  });

  it("accumulates run boundaries across polls instead of replacing them", async () => {
    // A begin and its end usually arrive in DIFFERENT replies. Replacing the
    // boundary list each poll would leave every run looking open forever.
    mockCallApi
      .mockResolvedValueOnce({
        ok: true,
        events: [],
        runs: [
          { kind: "run/begin", run_id: "r-a", ts_ms: T0, label: "the run" },
        ],
        cursor_ms: T0,
      })
      .mockResolvedValue({
        ok: true,
        events: [],
        runs: [
          { kind: "run/begin", run_id: "r-a", ts_ms: T0, label: "the run" },
          {
            kind: "run/end",
            run_id: "r-a",
            ts_ms: T0 + 5_000,
            outcome: "done",
          },
        ],
        cursor_ms: T0 + 5_000,
      });
    const { result } = renderHook(() => useOperationRuns(5_000));
    await flush();
    await tick();
    expect(result.current.boundaries.some((b) => b.kind === "run/end")).toBe(
      true,
    );
    // The re-sent begin is held once, not twice.
    expect(
      result.current.boundaries.filter((b) => b.kind === "run/begin"),
    ).toHaveLength(1);
  });

  it("never rewinds its cursor, even if a reply reports an older one", async () => {
    mockCallApi
      .mockResolvedValueOnce({
        ok: true,
        events: [],
        runs: [],
        cursor_ms: T0 + 5_000,
      })
      .mockResolvedValue({ ok: true, events: [], runs: [], cursor_ms: 0 });
    renderHook(() => useOperationRuns(5_000));
    await flush();
    await tick();
    expect(mockCallApi.mock.calls.length).toBeGreaterThan(1);
    expect(sinceOf(mockCallApi.mock.calls[1])).toBe(T0 + 5_000);
  });

  it("keeps the last good snapshot when a poll fails (no blanking)", async () => {
    mockCallApi
      .mockResolvedValueOnce({
        ok: true,
        events: [ev(T0, "survives")],
        runs: [],
        cursor_ms: T0,
      })
      .mockRejectedValue(new Error("HTTP 502"));
    const { result } = renderHook(() => useOperationRuns(5_000));
    await flush();
    expect(result.current.events).toHaveLength(1);
    await tick();
    await tick();
    expect(result.current.events).toHaveLength(1);
    expect(result.current.events[0].command).toBe("survives");
  });

  it("does not churn the snapshot when a poll brings nothing new", async () => {
    // The same reply arrives every few seconds against a mostly-static log. A
    // fresh array each time would re-derive every group and re-render the strip
    // (and the Path view around it) for no new fact, so the identity is kept.
    const payload = {
      ok: true,
      events: [ev(T0, "one")],
      runs: [{ kind: "run/begin", run_id: "r-a", ts_ms: T0 }],
      cursor_ms: T0,
    };
    mockCallApi.mockResolvedValue(payload);
    const { result } = renderHook(() => useOperationRuns(5_000));
    await flush();
    const first = result.current;
    await tick();
    await tick();
    expect(mockCallApi.mock.calls.length).toBeGreaterThan(2);
    expect(result.current).toBe(first);
  });

  it("stops polling once it is unmounted", async () => {
    // An in-flight reply must not reach setState on a dead component, and the
    // interval must not keep asking a device nobody is watching.
    mockCallApi.mockResolvedValue({
      ok: true,
      events: [],
      runs: [],
      cursor_ms: T0,
    });
    const { unmount } = renderHook(() => useOperationRuns(5_000));
    await flush();
    expect(mockCallApi).toHaveBeenCalled();
    unmount();
    const calls = mockCallApi.mock.calls.length;
    await tick();
    await tick();
    expect(mockCallApi.mock.calls.length).toBe(calls);
  });

  it("floors the cadence: a caller's shorter poll is the shared 5 s floor's (the module's rule)", async () => {
    // THE RULE THE MIGRATION INHERITS, pinned here because this hook is the one that used to run
    // raw `setInterval(tick, pollMs)`. A caller asking for 25 ms gets the module's floor instead —
    // the same rule useVitalsSeries and useMonitors had already stated by hand.
    mockCallApi.mockResolvedValue({
      ok: true,
      events: [],
      runs: [],
      cursor_ms: T0,
    });
    renderHook(() => useOperationRuns(25));
    await flush();
    expect(mockCallApi).toHaveBeenCalledTimes(1);
    await tick(4_999);
    expect(
      mockCallApi,
      "the floor, not the caller's 25 ms cadence",
    ).toHaveBeenCalledTimes(1);
    await tick(1);
    expect(mockCallApi).toHaveBeenCalledTimes(2);
  });

  it("re-reads when the tab becomes visible again (the one refresh `everyMs` cannot express)", async () => {
    mockCallApi.mockResolvedValue({
      ok: true,
      events: [],
      runs: [],
      cursor_ms: T0,
    });
    renderHook(() => useOperationRuns(60_000));
    await flush();
    expect(mockCallApi).toHaveBeenCalledTimes(1);
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
      await Promise.resolve();
    });
    expect(mockCallApi).toHaveBeenCalledTimes(2);
  });
});
