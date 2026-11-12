import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { EventSourceLike } from "./liveActivity";
import { useLiveActivity } from "./useLiveActivity";

/** The browser's `EventSource`, with the test deciding when a frame arrives. */
class FakeSource implements EventSourceLike {
  static instances: FakeSource[] = [];
  listeners = new Map<string, (event: MessageEvent) => void>();
  onerror: ((event: Event) => void) | null = null;
  constructor(public url: string) {
    FakeSource.instances.push(this);
  }
  addEventListener(type: string, listener: (event: MessageEvent) => void) {
    this.listeners.set(type, listener);
  }
  close() {}
  emit(type: string, data: unknown) {
    this.listeners.get(type)?.({ data: JSON.stringify(data) } as MessageEvent);
  }
}

const line = (ts: number, sentence: string) => ({ ts_ms: ts, kind: "mcp", status: 200, sentence });

/** One device event as the console relays it: the machine's id, then the journal's envelope. */
const deviceEvent = (
  type: string,
  ts: string,
  payload: Record<string, unknown> = { id: "2", kind: "pty" }
) => ({
  device_event: {
    device: "abc",
    event: { v: 1, plane: "control", seq: 7, ts, type, payload }
  }
});

beforeEach(() => {
  FakeSource.instances = [];
  vi.stubGlobal("EventSource", FakeSource);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("following the live activity", () => {
  it("counts a dropped frame and keeps the count until the reread answers", async () => {
    let answer!: () => void;
    const reread = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          answer = resolve;
        })
    );
    const { result } = renderHook(() => useLiveActivity(true, reread));
    const source = FakeSource.instances[0];

    act(() => {
      source.emit("lagged", { dropped: 3 });
    });

    // The frame's body is the hole's size, not a flag: the page has to be able to say how much it
    // missed, and a reread is already on its way.
    expect(result.current.lagged).toBe(3);
    await waitFor(() => expect(reread).toHaveBeenCalledTimes(1));

    act(() => {
      source.emit("activity", line(10, "Read `notes.txt`"));
    });
    // A line that resumed after the hole says nothing about the lines before it.
    expect(result.current.lagged).toBe(3);

    await act(async () => {
      answer();
    });
    await waitFor(() => expect(result.current.lagged).toBe(0));
  });

  it("counts the rows a journal reset lost and rereads the record once", async () => {
    let answer!: () => void;
    const reread = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          answer = resolve;
        })
    );
    const { result } = renderHook(() => useLiveActivity(true, reread));
    const source = FakeSource.instances[0];

    act(() => {
      source.emit("snapshot", { lines: [line(10, "Read `notes.txt`")] });
    });
    act(() => {
      // The journal dropped 41..999, so the page is missing 959 rows even though the stream is
      // healthy. It is the same hole `lagged` reports, and it takes the same reread.
      source.emit("reset", {
        device: "abc",
        snapshot_seq: 1000,
        lost: { from: 41, to: 999 }
      });
    });

    expect(result.current.lagged).toBe(959);
    // The reset does not clear what the page already has; only the reread's snapshot replaces it.
    expect(result.current.lines).toHaveLength(1);
    await waitFor(() => expect(reread).toHaveBeenCalledTimes(1));

    await act(async () => {
      answer();
    });
    await waitFor(() => expect(result.current.lagged).toBe(0));
    expect(reread).toHaveBeenCalledTimes(1);
  });

  it("adds a second dropped frame while the first reread is still in flight", async () => {
    const answers: Array<() => void> = [];
    const reread = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          answers.push(resolve);
        })
    );
    const { result } = renderHook(() => useLiveActivity(true, reread));
    const source = FakeSource.instances[0];

    act(() => {
      source.emit("lagged", { dropped: 3 });
      source.emit("lagged", { dropped: 2 });
    });
    expect(result.current.lagged).toBe(5);

    // The first answer covers only the drops that were known when that read began; the second drop
    // needs its own read before the count may fall.
    await waitFor(() => expect(reread).toHaveBeenCalledTimes(1));
    await act(async () => {
      answers[0]();
    });
    await waitFor(() => expect(reread).toHaveBeenCalledTimes(2));
    expect(result.current.lagged).toBe(5);

    await act(async () => {
      answers[1]();
    });
    await waitFor(() => expect(result.current.lagged).toBe(0));
  });

  it("does not let a normal line clear a count that no reread has answered", () => {
    const { result } = renderHook(() => useLiveActivity(true));
    const source = FakeSource.instances[0];

    act(() => {
      source.emit("lagged", { dropped: 3 });
    });
    act(() => {
      source.emit("activity", line(10, "Read `notes.txt`"));
    });

    // The stream is healthy again; the record is not, and only a reread may say otherwise.
    expect(result.current.lagged).toBe(3);
  });

  it("counts a snapshot that arrived after a dropped connection, and not the first", () => {
    const { result } = renderHook(() => useLiveActivity(true));
    const source = FakeSource.instances[0];

    act(() => {
      source.emit("snapshot", { lines: [] });
    });
    // Opening the stream is not a recovery: the first snapshot is the list, not a hole refilled.
    expect(result.current.recoveries).toBe(0);

    act(() => {
      source.onerror?.(new Event("error"));
    });
    act(() => {
      source.emit("snapshot", { lines: [line(10, "Ran the tests")] });
    });
    expect(result.current.recoveries).toBe(1);

    act(() => {
      source.emit("snapshot", { lines: [line(11, "Read the report")] });
    });
    expect(result.current.recoveries).toBe(1);
  });

  it("keeps a started call until its finished frame arrives", () => {
    const { result } = renderHook(() => useLiveActivity(true));
    const source = FakeSource.instances[0];
    const first = Date.now();

    act(() => {
      source.emit("started", {
        started: { id: "call-1", tool: "runs.exec", at_ms: first },
        caller: "client"
      });
    });
    expect(result.current.running).toEqual([
      { id: "call-1", tool: "runs.exec", at_ms: first, caller: "client" }
    ]);

    // The clear is by id: two calls can be running at once, and one finishing must not hide the
    // other. The same id is what the server's snapshot names.
    act(() => {
      source.emit("started", {
        started: { id: "call-2", tool: "files.read", at_ms: first + 1 },
        caller: "console"
      });
    });
    expect(result.current.running).toHaveLength(2);

    act(() => {
      source.emit("finished", { finished: { id: "call-1" } });
    });
    expect(result.current.running).toEqual([
      { id: "call-2", tool: "files.read", at_ms: first + 1, caller: "console" }
    ]);
  });

  it("replaces the running list with a snapshot's in-flight calls", () => {
    const { result } = renderHook(() => useLiveActivity(true));
    const source = FakeSource.instances[0];

    act(() => {
      source.emit("snapshot", {
        lines: [],
        running: [{ id: "call-9", tool: "runs.exec", at_ms: Date.now(), caller: "client" }]
      });
    });
    expect(result.current.running).toEqual([
      { id: "call-9", tool: "runs.exec", at_ms: expect.any(Number), caller: "client" }
    ]);

    // A reconnect's snapshot is the whole registry: a call cleared while the stream was down is not
    // kept from the previous list.
    act(() => {
      source.emit("snapshot", { lines: [], running: [] });
    });
    expect(result.current.running).toEqual([]);
  });

  it("draws a device event as a session line, not as a running call", () => {
    const { result } = renderHook(() => useLiveActivity(true));
    const source = FakeSource.instances[0];

    act(() => {
      source.emit("snapshot", { lines: [line(10, "Read `notes.txt`")] });
    });
    act(() => {
      source.emit("device", deviceEvent("session.opened", "2026-11-13T10:00:00.000Z"));
    });

    // The machine's journal is a row so a session opening is visible, but it is not one of the
    // proxy's calls: opening a terminal must not put anything in the running band.
    expect(result.current.lines).toEqual([
      {
        ts_ms: Date.parse("2026-11-13T10:00:00.000Z"),
        kind: "session",
        status: 200,
        ok: true,
        device_id: "abc",
        live: true,
        sentence: "terminal 2 opened on abc"
      },
      line(10, "Read `notes.txt`")
    ]);
    expect(result.current.running).toEqual([]);
  });

  it("names a session close, a started device, and a kind it does not know", () => {
    const { result } = renderHook(() => useLiveActivity(true));
    const source = FakeSource.instances[0];

    act(() => {
      source.emit("device", deviceEvent("session.closed", "2026-11-13T10:00:01.000Z"));
      source.emit("device", deviceEvent("device.started", "2026-11-13T10:00:02.000Z", {}));
      source.emit("device", deviceEvent("session.idle", "2026-11-13T10:00:03.000Z"));
    });

    // Newest first, like every other row. The two session kinds are the only ones the page can
    // describe; anything else says what it was rather than guessing what it did.
    expect(result.current.lines.map((entry) => entry.sentence)).toEqual([
      "session.idle on abc",
      "abc started",
      "terminal 2 closed on abc"
    ]);
  });

  it("dates a device event whose clock cannot be read at the page's own", () => {
    const before = Date.now();
    const { result } = renderHook(() => useLiveActivity(true));
    const source = FakeSource.instances[0];

    act(() => {
      source.emit("device", deviceEvent("session.opened", "not a time"));
    });

    // A device clock the page cannot parse is still an event that just arrived; the fallback is
    // newer than the event, never older.
    expect(result.current.lines[0].ts_ms).toBeGreaterThanOrEqual(before);
    expect(result.current.lines[0].ts_ms).toBeLessThanOrEqual(Date.now());
  });
});
