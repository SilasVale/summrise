import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HttpError, type AuditLine, type JoinAnswer } from "./api";

const mocks = vi.hoisted(() => ({ auditJoin: vi.fn() }));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return { ...actual, api: { ...actual.api, auditJoin: mocks.auditJoin } };
});

import { JOIN_CACHE_MS, JOIN_DEBOUNCE_MS, resetJoinMarks, useJoinMarks } from "./useJoinMarks";

const DEVICE = "d".repeat(64);

function line(trace: string | undefined, over: Partial<AuditLine> = {}): AuditLine {
  return {
    ts_ms: 1,
    kind: "mcp",
    device_id: DEVICE,
    status: 200,
    tool: "files.read",
    ...(trace === undefined ? {} : { trace }),
    ...over
  };
}

function answer(over: Partial<JoinAnswer> = {}): JoinAnswer {
  return {
    device: DEVICE,
    asked: 0,
    joined: [],
    missing: [],
    unknown: false,
    journal: true,
    bounded: false,
    oldest_seq: 1,
    last_seq: 9,
    read_at_ms: 1,
    ...over
  };
}

/** Past the debounce, without waiting for it. */
function settle() {
  act(() => {
    vi.advanceTimersByTime(JOIN_DEBOUNCE_MS);
  });
}

beforeEach(() => {
  resetJoinMarks();
  mocks.auditJoin.mockReset();
  vi.useFakeTimers({ shouldAdvanceTime: true });
  vi.setSystemTime(1_000_000);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useJoinMarks", () => {
  it("asks the machine about the window's traces after the burst settles", async () => {
    mocks.auditJoin.mockResolvedValue(answer({ asked: 2, joined: ["a", "b"] }));
    const hook = renderHook(() => useJoinMarks(DEVICE, [line("a"), line("b")]));

    // Nothing on the wire before the debounce: a burst of calls is one question.
    expect(mocks.auditJoin).not.toHaveBeenCalled();
    settle();
    await waitFor(() => expect(hook.result.current.kinds.get("a")).toBe("one"));
    expect(mocks.auditJoin).toHaveBeenCalledWith(DEVICE, ["a", "b"]);
  });

  it("asks nothing for the fleet's window", () => {
    // A fleet window spans machines, so there is no one journal to ask. The rows stay unknown, which
    // is the truth: the console did not check them.
    const state = renderHook(() => useJoinMarks(null, [line("a")])).result.current;
    settle();
    expect(mocks.auditJoin).not.toHaveBeenCalled();
    expect(state.traces).toEqual([]);
    expect(state.counts).toEqual({ one: 0, console: 0, unknown: 0, asked: 0 });
  });

  it("asks nothing when the window holds no call with a trace", () => {
    const state = renderHook(() =>
      useJoinMarks(DEVICE, [line(undefined), line(undefined, { kind: "grant" })])
    ).result.current;
    settle();
    expect(mocks.auditJoin).not.toHaveBeenCalled();
    expect(state.error).toBeNull();
  });

  it("does not ask a second time for a trace it already has an answer about", async () => {
    mocks.auditJoin.mockResolvedValue(answer({ asked: 1, joined: ["a"] }));
    const hook = renderHook(({ lines }) => useJoinMarks(DEVICE, lines), {
      initialProps: { lines: [line("a")] }
    });
    settle();
    await waitFor(() => expect(hook.result.current.kinds.get("a")).toBe("one"));

    // The same call arrives again on the live stream, then a new one: only the new trace is asked.
    mocks.auditJoin.mockResolvedValue(answer({ asked: 1, joined: ["b"] }));
    act(() => {
      hook.rerender({ lines: [line("a", { ts_ms: 2 }), line("b", { ts_ms: 3 })] });
    });
    settle();
    await waitFor(() => expect(hook.result.current.kinds.get("b")).toBe("one"));
    expect(mocks.auditJoin).toHaveBeenCalledTimes(2);
    expect(mocks.auditJoin).toHaveBeenLastCalledWith(DEVICE, ["b"]);
    // The first answer is not asked again, and the first row keeps its mark.
    expect(hook.result.current.kinds.get("a")).toBe("one");
  });

  it("marks a row console when the machine no longer holds it", async () => {
    mocks.auditJoin.mockResolvedValue(answer({ asked: 2, joined: ["a"], missing: ["b"] }));
    const hook = renderHook(() => useJoinMarks(DEVICE, [line("a"), line("b")]));
    settle();
    await waitFor(() => expect(hook.result.current.kinds.get("b")).toBe("console"));
    expect(hook.result.current.kinds.get("a")).toBe("one");
    expect(hook.result.current.counts).toEqual({ one: 1, console: 1, unknown: 0, asked: 2 });
  });

  it("marks nothing as lost when the read failed", async () => {
    // The one error this feature must not make: a console that could not ask drawing every row as
    // rotated out of a machine's journal.
    mocks.auditJoin.mockRejectedValue(new HttpError(503, "device_offline"));
    const hook = renderHook(() => useJoinMarks(DEVICE, [line("a")]));
    settle();
    await waitFor(() => expect(hook.result.current.error).not.toBeNull());
    expect(hook.result.current.error).toBe("the machine is not connected");
    expect(hook.result.current.kinds.get("a")).toBe("unknown");
    expect(hook.result.current.counts.console).toBe(0);
  });

  it("asks an offline machine once per answer window, not once per render", async () => {
    mocks.auditJoin.mockRejectedValue(new HttpError(503, "device_offline"));
    const hook = renderHook(({ lines }) => useJoinMarks(DEVICE, lines), {
      initialProps: { lines: [line("a")] }
    });
    settle();
    await waitFor(() => expect(hook.result.current.error).not.toBeNull());

    // The page keeps drawing the same rows while the machine is away. Each render is not a new
    // question: a failed read is remembered for as long as an answer would be.
    act(() => {
      hook.rerender({ lines: [line("a", { ts_ms: 7 })] });
    });
    settle();
    expect(mocks.auditJoin).toHaveBeenCalledTimes(1);
    expect(hook.result.current.kinds.get("a")).toBe("unknown");

    // Past the window the page asks again on its own — an offline machine is not frozen out for the
    // session, and the row it could not check today is checked again a half minute later.
    mocks.auditJoin.mockResolvedValue(answer({ asked: 1, joined: ["a"] }));
    act(() => {
      vi.advanceTimersByTime(JOIN_CACHE_MS);
    });
    settle();
    await waitFor(() => expect(hook.result.current.kinds.get("a")).toBe("one"));
    expect(mocks.auditJoin).toHaveBeenCalledTimes(2);
    expect(hook.result.current.error).toBeNull();
  });

  it("keeps an older machine's refusal out of the counts as unknown", async () => {
    mocks.auditJoin.mockResolvedValue(answer({ unknown: true, reason: "unknown_tool" }));
    const hook = renderHook(() => useJoinMarks(DEVICE, [line("a")]));
    settle();
    await waitFor(() => expect(hook.result.current.counts.unknown).toBe(1));
    expect(hook.result.current.kinds.get("a")).toBe("unknown");
    expect(hook.result.current.error).toBeNull();
  });

  it("asks again once the answer has aged past the cache window", async () => {
    mocks.auditJoin.mockResolvedValue(answer({ asked: 1, joined: ["a"] }));
    const hook = renderHook(({ lines }) => useJoinMarks(DEVICE, lines), {
      initialProps: { lines: [line("a")] }
    });
    settle();
    await waitFor(() => expect(hook.result.current.kinds.get("a")).toBe("one"));

    // The machine's journal rotates while the page stays open: no row changed, no render happened,
    // and the page must still ask again — the row that was joined gains its mark.
    mocks.auditJoin.mockResolvedValue(answer({ asked: 1, missing: ["a"] }));
    act(() => {
      vi.advanceTimersByTime(JOIN_CACHE_MS);
    });
    settle();
    await waitFor(() => expect(hook.result.current.kinds.get("a")).toBe("console"));
    expect(mocks.auditJoin).toHaveBeenCalledTimes(2);
  });

  it("names the console's sentence for a machine that did not answer in time", async () => {
    mocks.auditJoin.mockRejectedValue(new HttpError(504, "device_timeout"));
    const hook = renderHook(() => useJoinMarks(DEVICE, [line("a")]));
    settle();
    await waitFor(() => expect(hook.result.current.error).not.toBeNull());
    expect(hook.result.current.error).toBe("the machine did not answer in time");
  });
});
