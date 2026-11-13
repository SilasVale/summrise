import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HttpError, type Device, type MachineDayAnswer, type MachineReadAnswer } from "./api";

const mocks = vi.hoisted(() => ({ journalRead: vi.fn(), journalDay: vi.fn() }));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    api: { ...actual.api, journalRead: mocks.journalRead, journalDay: mocks.journalDay }
  };
});

import { FLEET_CACHE_MS, resetFleetRecord, useFleetRecord } from "./useFleetRecord";

const FROM = 1_000;
const TO = 2_000;

function device(id: string, over: Partial<Device> = {}): Device {
  return { id, name: id, online: true, version: null, last_seen_ms: null, ...over };
}

function page(deviceId: string): MachineReadAnswer {
  return {
    device: deviceId,
    journal: true,
    oldest_seq: 1,
    last_seq: 2,
    rows: [],
    more: false,
    read_at_ms: 1,
    unknown: false
  };
}

function day(deviceId: string): MachineDayAnswer {
  return {
    device: deviceId,
    journal: true,
    day_start_ms: FROM,
    day_end_ms: TO,
    oldest_seq: 1,
    last_seq: 2,
    scanned: 1,
    more: false,
    calls: {
      console: { calls: 0, failed: 0 },
      panel: { calls: 0, failed: 0 },
      mesh: { calls: 0, failed: 0 },
      loopback: { calls: 1, failed: 0 },
      unknown: { calls: 0, failed: 0 }
    },
    read_at_ms: 1,
    unknown: false
  };
}

beforeEach(() => {
  resetFleetRecord();
  mocks.journalRead.mockReset();
  mocks.journalDay.mockReset();
  mocks.journalRead.mockImplementation(async (id: string) => page(id));
  mocks.journalDay.mockImplementation(async (id: string) => day(id));
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useFleetRecord", () => {
  it("asks nothing when no window is named", () => {
    const { result } = renderHook(() => useFleetRecord([device("A")], null, null));
    expect(mocks.journalRead).not.toHaveBeenCalled();
    expect(mocks.journalDay).not.toHaveBeenCalled();
    expect(result.current.reads).toEqual([]);
  });

  it("asks each online machine once for its newest page and its day", async () => {
    const devices = [device("A"), device("B", { online: false }), device("C")];
    const { result } = renderHook(() => useFleetRecord(devices, FROM, TO));
    await waitFor(() =>
      expect(result.current.reads.every((read) => read.page.state === "read")).toBe(true)
    );
    expect(mocks.journalRead.mock.calls.map((call) => call[0])).toEqual(["A", "C"]);
    expect(mocks.journalRead.mock.calls[0]?.[1]).toEqual({ limit: 200 });
    expect(mocks.journalDay.mock.calls).toEqual([
      ["A", FROM, TO],
      ["C", FROM, TO]
    ]);
    expect(result.current.plan.offline.map((entry) => entry.id)).toEqual(["B"]);
    expect(result.current.reading).toBe(false);
  });

  it("asks at most eight machines and keeps the rest as the cap's fact", async () => {
    const devices = Array.from({ length: 10 }, (_, index) => device(`D${index}`));
    const { result } = renderHook(() => useFleetRecord(devices, FROM, TO));
    await waitFor(() => expect(result.current.reads).toHaveLength(8));
    expect(mocks.journalRead).toHaveBeenCalledTimes(8);
    expect(result.current.plan.beyond.map((entry) => entry.id)).toEqual(["D8", "D9"]);
  });

  it("waits on a read already in flight instead of asking a second time", async () => {
    let settle: ((answer: MachineReadAnswer) => void) | null = null;
    mocks.journalRead.mockImplementationOnce(
      (id: string) =>
        new Promise<MachineReadAnswer>((resolve) => {
          settle = resolve;
          void id;
        })
    );
    const first = renderHook(() => useFleetRecord([device("A")], FROM, TO));
    await waitFor(() => expect(first.result.current.reading).toBe(true));
    const second = renderHook(() => useFleetRecord([device("A")], FROM, TO));
    await waitFor(() => expect(second.result.current.reading).toBe(true));
    expect(mocks.journalRead).toHaveBeenCalledTimes(1);
    act(() => settle?.(page("A")));
    await waitFor(() => expect(first.result.current.reads[0]?.page.state).toBe("read"));
    expect(second.result.current.reads[0]?.page.state).toBe("read");
  });

  it("keeps a failed page apart from the day it still counted", async () => {
    mocks.journalRead.mockRejectedValue(new HttpError(503, "device_offline"));
    const { result } = renderHook(() => useFleetRecord([device("A")], FROM, TO));
    await waitFor(() => expect(result.current.reads[0]?.page.state).toBe("failed"));
    expect(result.current.reads[0]?.page.error).toBe("the machine is not connected");
    expect(result.current.reads[0]?.day.state).toBe("read");
    expect(result.current.reading).toBe(false);
  });

  it("asks again once the window has aged", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(1_000_000);
    renderHook(() => useFleetRecord([device("A")], FROM, TO));
    await waitFor(() => expect(mocks.journalRead).toHaveBeenCalledTimes(1));
    vi.advanceTimersByTime(FLEET_CACHE_MS + 10);
    await waitFor(() => expect(mocks.journalRead).toHaveBeenCalledTimes(2));
    expect(mocks.journalDay).toHaveBeenCalledTimes(2);
  });

  it("does not re-ask when the fleet's ids and the day block did not change", async () => {
    const { rerender } = renderHook(
      ({ devices }: { devices: Device[] }) => useFleetRecord(devices, FROM, TO),
      { initialProps: { devices: [device("A")] } }
    );
    await waitFor(() => expect(mocks.journalRead).toHaveBeenCalledTimes(1));
    // A poll answers a new array with the same machine: a new object is not a new window.
    rerender({ devices: [device("A")] });
    await act(async () => {});
    expect(mocks.journalRead).toHaveBeenCalledTimes(1);
  });

  it("asks the new block when the day moves", async () => {
    const { rerender } = renderHook(
      ({ to }: { to: number }) => useFleetRecord([device("A")], FROM, to),
      { initialProps: { to: TO } }
    );
    await waitFor(() => expect(mocks.journalDay).toHaveBeenCalledTimes(1));
    rerender({ to: TO + 86_400_000 });
    await waitFor(() => expect(mocks.journalDay).toHaveBeenCalledTimes(2));
    expect(mocks.journalDay.mock.calls[1]).toEqual(["A", FROM, TO + 86_400_000]);
  });

  it("starts clean after the test seam resets the cache", async () => {
    renderHook(() => useFleetRecord([device("A")], FROM, TO));
    await waitFor(() => expect(mocks.journalRead).toHaveBeenCalledTimes(1));
    resetFleetRecord();
    renderHook(() => useFleetRecord([device("A")], FROM, TO));
    await waitFor(() => expect(mocks.journalRead).toHaveBeenCalledTimes(2));
  });
});
