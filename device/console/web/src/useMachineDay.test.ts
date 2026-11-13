import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HttpError, type MachineDayAnswer } from "./api";

const mocks = vi.hoisted(() => ({ journalDay: vi.fn() }));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return { ...actual, api: { ...actual.api, journalDay: mocks.journalDay } };
});

import { MACHINE_DAY_CACHE_MS, resetMachineDays, useMachineDay } from "./useMachineDay";

const DEVICE = "d".repeat(64);
const FROM = 1_000;
const TO = 2_000;

function answer(over: Partial<MachineDayAnswer> = {}): MachineDayAnswer {
  return {
    device: DEVICE,
    journal: true,
    day_start_ms: FROM,
    day_end_ms: TO,
    oldest_seq: 3,
    last_seq: 9,
    scanned: 6,
    more: false,
    calls: {
      console: { calls: 1, failed: 0 },
      panel: { calls: 2, failed: 1 },
      mesh: { calls: 0, failed: 0 },
      loopback: { calls: 3, failed: 1 },
      unknown: { calls: 0, failed: 0 }
    },
    read_at_ms: 1,
    unknown: false,
    ...over
  };
}

beforeEach(() => {
  resetMachineDays();
  mocks.journalDay.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useMachineDay", () => {
  it("asks nothing when no machine or day is named", () => {
    const { result } = renderHook(() => useMachineDay(null, null, null));
    expect(result.current.state).toBe("idle");
    expect(mocks.journalDay).not.toHaveBeenCalled();
    const { result: noDay } = renderHook(() => useMachineDay(DEVICE, null, TO));
    expect(noDay.current.state).toBe("idle");
    expect(mocks.journalDay).not.toHaveBeenCalled();
  });

  it("asks for the two instants it counts and keeps the answer", async () => {
    mocks.journalDay.mockResolvedValue(answer());
    const { result, rerender } = renderHook(() => useMachineDay(DEVICE, FROM, TO));
    await waitFor(() => expect(result.current.state).toBe("read"));
    expect(mocks.journalDay).toHaveBeenCalledWith(DEVICE, FROM, TO);
    expect(result.current.answer?.calls.loopback).toEqual({ calls: 3, failed: 1 });
    rerender();
    await waitFor(() => expect(result.current.state).toBe("read"));
    expect(mocks.journalDay).toHaveBeenCalledTimes(1);
  });

  it("keeps a failed read apart from an empty day, and names the reason", async () => {
    mocks.journalDay.mockRejectedValue(new HttpError(503, "device_offline"));
    const { result } = renderHook(() => useMachineDay(DEVICE, FROM, TO));
    await waitFor(() => expect(result.current.state).toBe("failed"));
    expect(result.current.answer).toBeNull();
    expect(result.current.error).toBe("the machine is not connected");
  });

  it("passes an unknown answer through instead of inventing a zero day", async () => {
    mocks.journalDay.mockResolvedValue(answer({ unknown: true }));
    const { result } = renderHook(() => useMachineDay(DEVICE, FROM, TO));
    await waitFor(() => expect(result.current.state).toBe("read"));
    expect(result.current.answer?.unknown).toBe(true);
  });

  it("asks again once the day's answer has aged", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(1_000_000);
    mocks.journalDay.mockResolvedValue(answer());
    const { result } = renderHook(() => useMachineDay(DEVICE, FROM, TO));
    await waitFor(() => expect(result.current.state).toBe("read"));
    vi.advanceTimersByTime(MACHINE_DAY_CACHE_MS + 10);
    await waitFor(() => expect(mocks.journalDay).toHaveBeenCalledTimes(2));
  });

  it("waits on the read a remount interrupted rather than starting a second", async () => {
    let settle: (value: MachineDayAnswer) => void = () => {};
    mocks.journalDay.mockImplementation(
      () => new Promise<MachineDayAnswer>((resolve) => (settle = resolve))
    );
    const first = renderHook(() => useMachineDay(DEVICE, FROM, TO));
    await waitFor(() => expect(first.result.current.state).toBe("reading"));
    first.unmount();
    const second = renderHook(() => useMachineDay(DEVICE, FROM, TO));
    expect(second.result.current.state).toBe("reading");
    await act(async () => {
      settle(answer());
    });
    await waitFor(() => expect(second.result.current.state).toBe("read"));
    expect(mocks.journalDay).toHaveBeenCalledTimes(1);
  });
});
