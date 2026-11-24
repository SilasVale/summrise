import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HttpError, type MachineReadAnswer, type MachineRow } from "./api";

const mocks = vi.hoisted(() => ({ journalRead: vi.fn() }));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return { ...actual, api: { ...actual.api, journalRead: mocks.journalRead } };
});

import {
  JOURNAL_ROW_RETRY_MS,
  journalRowSentence,
  resetJournalRows,
  useJournalRow,
  type JournalRowState
} from "./useJournalRow";

const DEVICE = "d".repeat(64);

function row(over: Partial<MachineRow> = {}): MachineRow {
  return {
    seq: 814,
    ts: "2026-11-13T08:00:00.000Z",
    type: "call.finished",
    payload_bytes: 41,
    trace: "console-1",
    tool: "files.read",
    ok: true,
    door: "console",
    ...over
  };
}

function answer(over: Partial<MachineReadAnswer> = {}): MachineReadAnswer {
  return {
    device: DEVICE,
    journal: true,
    oldest_seq: 700,
    last_seq: 900,
    rows: [row()],
    more: false,
    read_at_ms: 1,
    unknown: false,
    ...over
  };
}

function state(over: Partial<JournalRowState> = {}): JournalRowState {
  return { state: "read", row: row(), answer: answer(), error: null, ...over };
}

beforeEach(() => {
  resetJournalRows();
  mocks.journalRead.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useJournalRow", () => {
  it("reads the one row a sequence names", async () => {
    mocks.journalRead.mockResolvedValue(answer());
    const { result } = renderHook(() => useJournalRow(DEVICE, 814));
    await waitFor(() => expect(result.current.state).toBe("read"));
    expect(mocks.journalRead).toHaveBeenCalledWith(DEVICE, { before: 815, limit: 1 });
    expect(result.current.row?.seq).toBe(814);
  });

  it("asks nothing when no position is named", () => {
    const { result } = renderHook(() => useJournalRow(null, null));
    expect(result.current.state).toBe("idle");
    expect(mocks.journalRead).not.toHaveBeenCalled();
  });

  it("remembers the row: a sequence names one row forever", async () => {
    mocks.journalRead.mockResolvedValue(answer());
    const { result, rerender } = renderHook(() => useJournalRow(DEVICE, 814));
    await waitFor(() => expect(result.current.state).toBe("read"));
    rerender();
    await waitFor(() => expect(result.current.state).toBe("read"));
    expect(mocks.journalRead).toHaveBeenCalledTimes(1);
  });

  it("does not hand back a different call's row", async () => {
    mocks.journalRead.mockResolvedValue(answer({ rows: [row({ seq: 815 })] }));
    const { result } = renderHook(() => useJournalRow(DEVICE, 814));
    await waitFor(() => expect(result.current.state).toBe("read"));
    expect(result.current.row).toBeNull();
  });

  it("does not accept a row whose trace names another call", async () => {
    // The bus has no per-journal epoch yet, so a reset can make an old line's sequence name a
    // different call; the trace is the second half of the question.
    mocks.journalRead.mockResolvedValue(answer({ rows: [row({ trace: "console-other" })] }));
    const { result } = renderHook(() => useJournalRow(DEVICE, 814, "console-1"));
    await waitFor(() => expect(result.current.state).toBe("read"));
    expect(result.current.row).toBeNull();
  });

  it("keeps two devices' identical sequences apart", async () => {
    const other = "e".repeat(64);
    mocks.journalRead.mockImplementation(async (device: string) =>
      answer({ device, rows: [row({ trace: "console-1" })] })
    );
    const { result } = renderHook(() => ({
      one: useJournalRow(DEVICE, 814, "console-1"),
      two: useJournalRow(other, 814, "console-1")
    }));
    await waitFor(() => expect(result.current.one.state).toBe("read"));
    await waitFor(() => expect(result.current.two.state).toBe("read"));
    expect(mocks.journalRead).toHaveBeenCalledTimes(2);
    expect(mocks.journalRead).toHaveBeenCalledWith(other, { before: 815, limit: 1 });
  });

  it("reads again when the opened row's sequence changes", async () => {
    mocks.journalRead.mockImplementation(async (_device: string, options: { before: number }) =>
      answer({ rows: [row({ seq: options.before - 1 })] })
    );
    const { result, rerender } = renderHook(
      ({ seq }: { seq: number }) => useJournalRow(DEVICE, seq, "console-1"),
      { initialProps: { seq: 814 } }
    );
    await waitFor(() => expect(result.current.state).toBe("read"));
    rerender({ seq: 800 });
    await waitFor(() => expect(mocks.journalRead).toHaveBeenCalledTimes(2));
    expect(mocks.journalRead).toHaveBeenLastCalledWith(DEVICE, { before: 801, limit: 1 });
  });

  it("maps an unknown answer and a journal-less machine to no row, not a failure", async () => {
    mocks.journalRead.mockResolvedValue(answer({ unknown: true, rows: [] }));
    const unknown = renderHook(() => useJournalRow(DEVICE, 814, "console-1"));
    await waitFor(() => expect(unknown.result.current.state).toBe("read"));
    expect(unknown.result.current.row).toBeNull();
    expect(unknown.result.current.answer?.unknown).toBe(true);
    unknown.unmount();

    mocks.journalRead.mockResolvedValue(answer({ journal: false, rows: [] }));
    const noJournal = renderHook(() => useJournalRow(DEVICE, 815, "console-1"));
    await waitFor(() => expect(noJournal.result.current.state).toBe("read"));
    expect(noJournal.result.current.row).toBeNull();
    expect(noJournal.result.current.answer?.journal).toBe(false);
  });

  it("lets a remount wait on the read already in flight", async () => {
    let settle: (value: MachineReadAnswer) => void = () => {};
    mocks.journalRead.mockImplementation(
      () => new Promise<MachineReadAnswer>((resolve) => (settle = resolve))
    );
    const first = renderHook(() => useJournalRow(DEVICE, 814, "console-1"));
    await waitFor(() => expect(first.result.current.state).toBe("reading"));
    first.unmount();
    const second = renderHook(() => useJournalRow(DEVICE, 814, "console-1"));
    expect(second.result.current.state).toBe("reading");
    await act(async () => {
      settle(answer());
    });
    await waitFor(() => expect(second.result.current.state).toBe("read"));
    expect(mocks.journalRead).toHaveBeenCalledTimes(1);
  });

  it("reuses a success when the record is closed and reopened", async () => {
    mocks.journalRead.mockResolvedValue(answer());
    const first = renderHook(() => useJournalRow(DEVICE, 814, "console-1"));
    await waitFor(() => expect(first.result.current.state).toBe("read"));
    first.unmount();
    const second = renderHook(() => useJournalRow(DEVICE, 814, "console-1"));
    expect(second.result.current.state).toBe("read");
    expect(second.result.current.row?.seq).toBe(814);
    expect(mocks.journalRead).toHaveBeenCalledTimes(1);
  });

  it("asks again when a record is reopened after a failed read has aged", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(1_000_000);
    mocks.journalRead.mockRejectedValueOnce(new HttpError(503, "device_offline"));
    mocks.journalRead.mockResolvedValue(answer());
    const first = renderHook(() => useJournalRow(DEVICE, 814));
    await waitFor(() => expect(first.result.current.state).toBe("failed"));
    first.unmount();

    // A failure is about the moment, not the row: reopening immediately trusts the sentence...
    const soon = renderHook(() => useJournalRow(DEVICE, 814));
    await waitFor(() => expect(soon.result.current.state).toBe("failed"));
    expect(mocks.journalRead).toHaveBeenCalledTimes(1);
    soon.unmount();

    // ...and reopening after the retry window asks again.
    vi.advanceTimersByTime(JOURNAL_ROW_RETRY_MS + 1);
    const later = renderHook(() => useJournalRow(DEVICE, 814));
    await waitFor(() => expect(later.result.current.state).toBe("read"));
    expect(mocks.journalRead).toHaveBeenCalledTimes(2);
  });

  it("keeps a failed read apart from a rotated row", async () => {
    mocks.journalRead.mockRejectedValue(new HttpError(503, "device_offline"));
    const { result } = renderHook(() => useJournalRow(DEVICE, 814));
    await waitFor(() => expect(result.current.state).toBe("failed"));
    expect(result.current.error).toBe("the machine is not connected");
  });
});

describe("journalRowSentence", () => {
  it("states the machine's own facts for the row it found", () => {
    expect(journalRowSentence(state(), 814)).toBe(
      "The machine's own row 814: call.finished at 2026-11-13T08:00:00.000Z, files.read ok, arrived by console, payload 41 B."
    );
    expect(
      journalRowSentence(
        state({
          row: row({ type: "call.denied", ok: false, reason: "denied", tool: "files.write" })
        }),
        814
      )
    ).toContain("files.write refused: denied");
  });

  it("says a rotated row is gone, not that the call did not happen", () => {
    const rotated = state({ row: null, answer: answer({ rows: [] }) });
    expect(journalRowSentence(rotated, 814)).toBe(
      "The machine no longer holds this call at row 814; its journal has rotated past it or been reset."
    );
  });

  it("keeps an unknown answer and a journal-less machine apart from a rotated row", () => {
    expect(
      journalRowSentence(state({ row: null, answer: answer({ unknown: true, rows: [] }) }), 814)
    ).toContain("this machine does not offer its journal");
    expect(
      journalRowSentence(state({ row: null, answer: answer({ journal: false, rows: [] }) }), 814)
    ).toContain("this machine keeps no journal");
  });

  it("says why a read failed", () => {
    const failed = state({
      state: "failed",
      row: null,
      answer: null,
      error: "the machine is not connected"
    });
    expect(journalRowSentence(failed, 814)).toBe(
      "The machine's own row 814 could not be read: the machine is not connected."
    );
  });

  it("says nothing while the read is still out", () => {
    expect(journalRowSentence(state({ state: "reading", row: null, answer: null }), 814)).toBeNull();
  });
});
