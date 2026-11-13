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

  it("pages older from the oldest sequence fetched, and appends", async () => {
    mocks.journalRead.mockImplementation(
      async (_device: string, options: { before?: number }) =>
        options.before === undefined
          ? answer({ rows: [row({ seq: 10 }), row({ seq: 9 })], more: true })
          : answer({ rows: [row({ seq: 8 }), row({ seq: 7 })], more: false })
    );
    const { result } = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(result.current.state).toBe("read"));
    expect(result.current.more).toBe(true);
    expect(result.current.paged).toBe(false);

    act(() => result.current.loadOlder());
    await waitFor(() => expect(result.current.more).toBe(false));
    expect(mocks.journalRead).toHaveBeenLastCalledWith(DEVICE, { before: 9, limit: 200 });
    expect(result.current.rows.map((line) => line.machine_seq)).toEqual([10, 9, 8, 7]);
    expect(result.current.paged).toBe(true);
    expect(result.current.readingOlder).toBe(false);
  });

  it("does not draw a sequence twice when a page overlaps", async () => {
    mocks.journalRead.mockImplementation(
      async (_device: string, options: { before?: number }) =>
        options.before === undefined
          ? answer({ rows: [row({ seq: 10 }), row({ seq: 9 })], more: true })
          : answer({ rows: [row({ seq: 9 }), row({ seq: 8 })], more: false })
    );
    const { result } = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(result.current.state).toBe("read"));
    act(() => result.current.loadOlder());
    await waitFor(() => expect(result.current.more).toBe(false));
    expect(result.current.rows.map((line) => line.machine_seq)).toEqual([10, 9, 8]);
  });

  it("asks once while an older page is in flight", async () => {
    let settle: (value: MachineReadAnswer) => void = () => {};
    mocks.journalRead.mockImplementation(async (_device: string, options: { before?: number }) => {
      if (options.before === undefined) {
        return answer({ rows: [row({ seq: 10 })], more: true });
      }
      return new Promise<MachineReadAnswer>((resolve) => (settle = resolve));
    });
    const { result } = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(result.current.state).toBe("read"));
    act(() => result.current.loadOlder());
    await waitFor(() => expect(result.current.readingOlder).toBe(true));
    act(() => result.current.loadOlder());
    expect(mocks.journalRead).toHaveBeenCalledTimes(2);
    await act(async () => {
      settle(answer({ rows: [row({ seq: 9 })], more: false }));
    });
    await waitFor(() => expect(result.current.readingOlder).toBe(false));
  });

  it("keeps the rows and offers a retry when an older page fails", async () => {
    let second = false;
    mocks.journalRead.mockImplementation(async (_device: string, options: { before?: number }) => {
      if (options.before === undefined) return answer({ rows: [row({ seq: 10 })], more: true });
      second = true;
      throw new HttpError(503, "device_offline");
    });
    const { result } = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(result.current.state).toBe("read"));
    act(() => result.current.loadOlder());
    await waitFor(() => expect(result.current.olderError).toBe("the machine is not connected"));
    expect(result.current.rows).toHaveLength(1);
    expect(result.current.more).toBe(true);

    // The retry continues from the same sequence, and the rows already drawn stay.
    mocks.journalRead.mockImplementation(async (_device: string, options: { before?: number }) =>
      options.before === undefined
        ? answer({ rows: [row({ seq: 10 })], more: true })
        : answer({ rows: [row({ seq: 9 })], more: false })
    );
    act(() => result.current.loadOlder());
    await waitFor(() => expect(result.current.rows).toHaveLength(2));
    expect(mocks.journalRead).toHaveBeenLastCalledWith(DEVICE, { before: 10, limit: 200 });
    expect(result.current.olderError).toBeNull();
    expect(second).toBe(true);
  });

  it("says an older page the machine cannot answer for, and keeps what it drew", async () => {
    mocks.journalRead.mockImplementation(async (_device: string, options: { before?: number }) =>
      options.before === undefined
        ? answer({ rows: [row({ seq: 10 })], more: true })
        : answer({ unknown: true, rows: [] })
    );
    const { result } = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(result.current.state).toBe("read"));
    act(() => result.current.loadOlder());
    await waitFor(() => expect(result.current.olderError).toBe("this machine does not offer its journal"));
    expect(result.current.rows).toHaveLength(1);
    expect(result.current.more).toBe(true);
  });

  it("does not ask when the machine has no older row", async () => {
    mocks.journalRead.mockResolvedValue(answer({ rows: [row({ seq: 10 })], more: false }));
    const { result } = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(result.current.state).toBe("read"));
    act(() => result.current.loadOlder());
    expect(mocks.journalRead).toHaveBeenCalledTimes(1);
    expect(result.current.paged).toBe(false);
  });

  it("starts a fresh session when the machine window is left and entered again", async () => {
    mocks.journalRead.mockImplementation(async (_device: string, options: { before?: number }) =>
      options.before === undefined
        ? answer({ rows: [row({ seq: 10 })], more: true })
        : answer({ rows: [row({ seq: 9 })], more: false })
    );
    const { result, rerender } = renderHook(
      ({ id }: { id: string | null }) => useMachineRows(id, "Alpha"),
      { initialProps: { id: DEVICE as string | null } }
    );
    await waitFor(() => expect(result.current.state).toBe("read"));
    act(() => result.current.loadOlder());
    await waitFor(() => expect(result.current.rows).toHaveLength(2));

    // Leaving for the fleet and coming back starts from the newest page: the pages asked for belong
    // to the window they were read for.
    rerender({ id: null });
    rerender({ id: DEVICE });
    await waitFor(() => expect(result.current.paged).toBe(false));
    expect(result.current.rows.map((line) => line.machine_seq)).toEqual([10]);
    expect(result.current.more).toBe(true);
    expect(mocks.journalRead).toHaveBeenLastCalledWith(DEVICE, { limit: 200 });
    // The fleet read and the re-entry's newest read: nothing else.
    expect(mocks.journalRead).toHaveBeenCalledTimes(3);
  });

  it("does not resurrect a session whose window was left mid-page", async () => {
    let settle: (value: MachineReadAnswer) => void = () => {};
    mocks.journalRead.mockImplementation(async (_device: string, options: { before?: number }) => {
      if (options.before === undefined) return answer({ rows: [row({ seq: 10 })], more: true });
      return new Promise<MachineReadAnswer>((resolve) => (settle = resolve));
    });
    const { result, rerender } = renderHook(
      ({ id }: { id: string | null }) => useMachineRows(id, "Alpha"),
      { initialProps: { id: DEVICE as string | null } }
    );
    await waitFor(() => expect(result.current.state).toBe("read"));
    act(() => result.current.loadOlder());
    await waitFor(() => expect(result.current.readingOlder).toBe(true));

    // Leave the machine window while the older page is on the wire, and let it land afterwards.
    rerender({ id: null });
    await act(async () => {
      settle(answer({ rows: [row({ seq: 9 })], more: false }));
    });
    rerender({ id: DEVICE });
    await waitFor(() => expect(result.current.state).toBe("read"));
    expect(result.current.rows.map((line) => line.machine_seq)).toEqual([10]);
    expect(result.current.paged).toBe(false);
    expect(result.current.readingOlder).toBe(false);
    expect(mocks.journalRead).toHaveBeenCalledTimes(2);
  });

  it("keeps a window the machine later says it cannot answer for", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(4_000_000);
    mocks.journalRead.mockImplementation(async (_device: string, options: { before?: number }) =>
      options.before === undefined
        ? answer({ rows: [row({ seq: 10 })], more: true })
        : answer({ rows: [row({ seq: 9 })], more: false })
    );
    const { result } = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(result.current.state).toBe("read"));
    act(() => result.current.loadOlder());
    await waitFor(() => expect(result.current.rows).toHaveLength(2));

    // A build change answers `unknown` with a 200. The rows were read and stay drawn; the footer's
    // "not read" sentence is for a read that never had a window.
    mocks.journalRead.mockResolvedValue(answer({ unknown: true, rows: [] }));
    vi.advanceTimersByTime(MACHINE_ROWS_CACHE_MS + 10);
    await waitFor(() => expect(mocks.journalRead).toHaveBeenCalledTimes(3));
    await waitFor(() => expect(result.current.state).toBe("read"));
    expect(result.current.rows.map((line) => line.machine_seq)).toEqual([10, 9]);
    expect(result.current.answer?.unknown).not.toBe(true);
    expect(result.current.more).toBe(false);
  });

  it("does not postpone the newest page's refresh when an older page lands", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(5_000_000);
    mocks.journalRead.mockImplementation(async (_device: string, options: { before?: number }) =>
      options.before === undefined
        ? answer({ rows: [row({ seq: 10 })], more: true })
        : answer({ rows: [row({ seq: 9 })], more: false })
    );
    const { result } = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(result.current.state).toBe("read"));
    vi.advanceTimersByTime(20_000);
    act(() => result.current.loadOlder());
    await waitFor(() => expect(result.current.rows).toHaveLength(2));
    // 20 s of the 30 s window were used before paging; 10 s remained, and 11 s later the page is due.
    vi.advanceTimersByTime(11_000);
    await waitFor(() =>
      expect(
        mocks.journalRead.mock.calls.filter((call) => call[1]?.before === undefined)
      ).toHaveLength(2)
    );
  });

  it("refreshes the newest page without dropping the pages asked for", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(2_000_000);
    mocks.journalRead.mockImplementation(async (_device: string, options: { before?: number }) => {
      if (options.before === undefined) {
        return answer({ rows: [row({ seq: 10 }), row({ seq: 9 })], more: true });
      }
      return answer({ rows: [row({ seq: 8 }), row({ seq: 7 })], more: false });
    });
    const { result } = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(result.current.state).toBe("read"));
    act(() => result.current.loadOlder());
    await waitFor(() => expect(result.current.more).toBe(false));

    mocks.journalRead.mockImplementation(async (_device: string, options: { before?: number }) =>
      options.before === undefined
        ? answer({ rows: [row({ seq: 12 }), row({ seq: 11 })], more: true })
        : answer({ rows: [row({ seq: 8 }), row({ seq: 7 })], more: false })
    );
    vi.advanceTimersByTime(MACHINE_ROWS_CACHE_MS + 10);
    await waitFor(() => expect(result.current.rows).toHaveLength(6));
    expect(result.current.rows.map((line) => line.machine_seq)).toEqual([12, 11, 10, 9, 8, 7]);
    expect(result.current.more).toBe(false);
    expect(result.current.paged).toBe(true);
  });

  it("keeps an unknown answer apart from an empty one", async () => {
    mocks.journalRead.mockResolvedValue(answer({ unknown: true, rows: [] }));
    const { result } = renderHook(() => useMachineRows(DEVICE, "Alpha"));
    await waitFor(() => expect(result.current.state).toBe("read"));
    expect(result.current.answer?.unknown).toBe(true);
    expect(result.current.rows).toEqual([]);
  });
});
