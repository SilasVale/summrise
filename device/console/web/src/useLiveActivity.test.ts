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
});
