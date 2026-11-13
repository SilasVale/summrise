import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HttpError, type MachineReadAnswer, type MachineRow } from "./api";

const mocks = vi.hoisted(() => ({ journalRead: vi.fn() }));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return { ...actual, api: { ...actual.api, journalRead: mocks.journalRead } };
});

import { MACHINE_ROWS_CACHE_MS, resetMachineRows, useMachineRows } from "./useMachineRows";

const DEVICE = "d".repeat(64);
const OTHER = "e".repeat(64);

function row(over: Partial<MachineRow> = {}): MachineRow {
  return {
    seq: 10,
    ts: "2026-11-05T09:00:00.000Z",
    type: "call.finished",
    payload_bytes: 10,
    trace: "machine-1",
    tool: "files.read",
    ok: true,
    door: "panel",
    ...over
  };
}

function answer(over: Partial<MachineReadAnswer> = {}): MachineReadAnswer {
  return {
    device: DEVICE,
    journal: true,
    oldest_seq: 1,
    last_seq: 10,
    rows: [row()],
    more: false,
    read_at_ms: 1,
    unknown: false,
    ...over
  };
}

beforeEach(() => {
  resetMachineRows();
  mocks.journalRead.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useMachineRows", () => {
  it("asks nothing for a fleet window", () => {
    const { result } = renderHook(() => useMachineRows(null));
    expect(result.current.state).toBe("idle");
    expect(result.current.rows).toEqual([]);
    expect(mocks.journalRead).not.toHaveBeenCalled();
  });

  it("reads one machine's newest window once", async () => {
    mocks.journalRead.mockResolvedValue(answer());
    const { result, rerender } = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(result.current.state).toBe("read"));
    expect(mocks.journalRead).toHaveBeenCalledTimes(1);
    expect(mocks.journalRead).toHaveBeenCalledWith(DEVICE, { limit: 200 });
    expect(result.current.rows[0].sentence).toBe("files.read ran on Alpha");
    rerender();
    await waitFor(() => expect(result.current.state).toBe("read"));
    expect(mocks.journalRead).toHaveBeenCalledTimes(1);
  });

  it("keeps each machine's window apart", async () => {
    mocks.journalRead.mockResolvedValue(answer());
    const { result, rerender } = renderHook(({ id }: { id: string }) => useMachineRows(id, "Alpha"), {
      initialProps: { id: DEVICE }
    });
    await waitFor(() => expect(result.current.state).toBe("read"));
    rerender({ id: OTHER });
    await waitFor(() => expect(mocks.journalRead).toHaveBeenCalledTimes(2));
    expect(mocks.journalRead).toHaveBeenLastCalledWith(OTHER, { limit: 200 });
  });

  it("asks again once the window has aged", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(1_000_000);
    mocks.journalRead.mockResolvedValue(answer());
    const { result } = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(result.current.state).toBe("read"));
    vi.advanceTimersByTime(MACHINE_ROWS_CACHE_MS + 10);
    await waitFor(() => expect(mocks.journalRead).toHaveBeenCalledTimes(2));
  });

  it("keeps a failed read apart from an empty journal, and names the reason", async () => {
    mocks.journalRead.mockRejectedValue(new HttpError(503, "device_offline"));
    const { result } = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(result.current.state).toBe("failed"));
    expect(result.current.rows).toEqual([]);
    expect(result.current.error).toBe("the machine is not connected");
  });

  it("waits on the read a remount interrupted rather than starting a second", async () => {
    let settle: (value: MachineReadAnswer) => void = () => {};
    mocks.journalRead.mockImplementation(
      () => new Promise<MachineReadAnswer>((resolve) => (settle = resolve))
    );
    const first = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(first.result.current.state).toBe("reading"));
    first.unmount();
    const second = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    expect(second.result.current.state).toBe("reading");
    await act(async () => {
      settle(answer());
    });
    await waitFor(() => expect(second.result.current.state).toBe("read"));
    expect(mocks.journalRead).toHaveBeenCalledTimes(1);
  });

  it("keeps an unknown answer apart from an empty one", async () => {
    mocks.journalRead.mockResolvedValue(answer({ unknown: true, rows: [] }));
    const { result } = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(result.current.state).toBe("read"));
    expect(result.current.answer?.unknown).toBe(true);
    expect(result.current.rows).toEqual([]);
  });
});
