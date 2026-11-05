import { describe, expect, it, vi } from "vitest";
import { mergeActivity } from "./activityMerge";
import { liveActivityPath, openLiveActivity, type EventSourceLike } from "./liveActivity";

class FakeSource implements EventSourceLike {
  listeners = new Map<string, (event: MessageEvent) => void>();
  closed = false;
  onerror: ((event: Event) => void) | null = null;
  constructor(public url: string) {}
  addEventListener(type: string, listener: (event: MessageEvent) => void) {
    this.listeners.set(type, listener);
  }
  close() {
    this.closed = true;
  }
  emit(type: string, data: unknown) {
    this.listeners.get(type)?.({ data: JSON.stringify(data) } as MessageEvent);
  }
  fail() {
    this.onerror?.(new Event("error"));
  }
}

const line = (ts: number, sentence: string) => ({ ts_ms: ts, kind: "mcp", status: 200, ok: true, sentence });

describe("the live activity stream", () => {
  it("opens the console's route", () => {
    expect(liveActivityPath()).toBe("/events");
  });

  it("replaces the list with each snapshot", () => {
    const source = new FakeSource("/events");
    const handlers = { snapshot: vi.fn(), activity: vi.fn(), lagged: vi.fn(), error: vi.fn() };
    const feed = openLiveActivity(handlers, () => source);
    expect(feed).not.toBeNull();
    source.emit("snapshot", { lines: [line(1, "Read `a.txt`"), line(2, "Issued a grant")] });
    expect(handlers.snapshot).toHaveBeenCalledWith(
      [
        { ts_ms: 1, kind: "mcp", status: 200, ok: true, sentence: "Read `a.txt`" },
        { ts_ms: 2, kind: "mcp", status: 200, ok: true, sentence: "Issued a grant" }
      ],
      []
    );
  });

  it("keeps a snapshot row's token through the merge", () => {
    // The snapshot is the newest 40 lines, and the merge shows the live copy of a line the fetched
    // window also holds. If the snapshot dropped the token, the newest rows would say "—" while the
    // `/audit` copy behind them names it.
    const source = new FakeSource("/events");
    const handlers = { snapshot: vi.fn(), activity: vi.fn(), lagged: vi.fn(), error: vi.fn() };
    openLiveActivity(handlers, () => source);
    const token = "a".repeat(64);
    source.emit("snapshot", {
      lines: [
        {
          ...line(5, "Read notes.txt"),
          tool: "files.read",
          token_id: token,
          caller: "client"
        }
      ]
    });
    const snapshot = handlers.snapshot.mock.calls[0][0];
    expect(snapshot[0].token_id).toBe(token);
    const merged = mergeActivity([], snapshot);
    expect(merged[0].token_id).toBe(token);
    expect(merged[0].caller).toBe("client");
  });

  it("hands one new line to activity", () => {
    const source = new FakeSource("/events");
    const handlers = { snapshot: vi.fn(), activity: vi.fn(), lagged: vi.fn(), error: vi.fn() };
    openLiveActivity(handlers, () => source);
    source.emit("activity", {
      ...line(3, "Ran `cargo test`"),
      device_id: "abc",
      tool: "runs.exec",
      ms: 120,
      live: true
    });
    expect(handlers.activity).toHaveBeenCalledWith({
      ts_ms: 3, kind: "mcp", status: 200, ok: true, sentence: "Ran `cargo test`", device_id: "abc", tool: "runs.exec", ms: 120, live: true
    });
  });

  it("drops a line that has no sentence, because a page cannot describe it", () => {
    const source = new FakeSource("/events");
    const handlers = { snapshot: vi.fn(), activity: vi.fn(), lagged: vi.fn(), error: vi.fn() };
    openLiveActivity(handlers, () => source);
    source.emit("activity", { ts_ms: 4, kind: "mcp", status: 200 });
    expect(handlers.activity).not.toHaveBeenCalled();
  });

  it("hands the dropped-line count to the page, and reports a dropped connection", () => {
    const source = new FakeSource("/events");
    const handlers = { snapshot: vi.fn(), activity: vi.fn(), lagged: vi.fn(), error: vi.fn() };
    openLiveActivity(handlers, () => source);
    source.emit("lagged", { dropped: 3 });
    source.fail();
    // The hole is a number the page can say out loud; discarding the frame body is what let the
    // page keep its "Following" sentence over it.
    expect(handlers.lagged).toHaveBeenCalledWith(3);
    expect(handlers.error).toHaveBeenCalledOnce();
  });

  it("says when a snapshot arrived after the connection dropped, and only then", () => {
    const source = new FakeSource("/events");
    const handlers = {
      snapshot: vi.fn(),
      activity: vi.fn(),
      lagged: vi.fn(),
      error: vi.fn(),
      recovered: vi.fn()
    };
    openLiveActivity(handlers, () => source);

    // The first snapshot opens the stream; it is not a recovery from anything.
    source.emit("snapshot", { lines: [line(1, "Read `a.txt`")] });
    expect(handlers.recovered).not.toHaveBeenCalled();

    source.fail();
    source.emit("snapshot", { lines: [line(2, "Wrote `b.txt`")] });
    // The reconnect's snapshot replaced the list; the page is told once so it can reread the gap.
    expect(handlers.recovered).toHaveBeenCalledOnce();

    // Later snapshots on the healthy connection are not new recoveries.
    source.emit("snapshot", { lines: [line(3, "Read `c.txt`")] });
    expect(handlers.recovered).toHaveBeenCalledOnce();
  });

  it("closes the connection once, and is null where the runtime has no EventSource", () => {
    const source = new FakeSource("/events");
    const handlers = { snapshot: vi.fn(), activity: vi.fn(), lagged: vi.fn(), error: vi.fn() };
    const feed = openLiveActivity(handlers, () => source)!;
    feed.close();
    feed.close();
    expect(source.closed).toBe(true);
    expect(openLiveActivity(handlers, null)).toBeNull();
  });
  it("hands a started frame to the page with its caller, and a finished frame its id", () => {
    const source = new FakeSource("/events");
    const handlers = {
      snapshot: vi.fn(),
      activity: vi.fn(),
      lagged: vi.fn(),
      error: vi.fn(),
      started: vi.fn(),
      finished: vi.fn()
    };
    openLiveActivity(handlers, () => source);

    // The frame the proxy publishes before the device answers: the shape is the announcement, and
    // the caller is top-level because the band filters on it.
    source.emit("started", {
      started: {
        device_id: "abc",
        tool: "runs.exec",
        id: "call-7",
        at_ms: 1_000
      },
      caller: "client"
    });
    expect(handlers.started).toHaveBeenCalledWith({
      device_id: "abc",
      tool: "runs.exec",
      id: "call-7",
      at_ms: 1_000,
      caller: "client"
    });

    source.emit("finished", { finished: { id: "call-7" } });
    expect(handlers.finished).toHaveBeenCalledWith("call-7");
  });

  it("replaces the running list with each snapshot's in-flight calls", () => {
    const source = new FakeSource("/events");
    const handlers = { snapshot: vi.fn(), activity: vi.fn(), lagged: vi.fn(), error: vi.fn() };
    openLiveActivity(handlers, () => source);

    // A reconnect must not forget a call that is still running, and must not keep one it missed the
    // clear for: the registry's snapshot is the whole list, not an increment.
    source.emit("snapshot", {
      lines: [],
      running: [{ id: "call-8", tool: "files.read", at_ms: 2_000, caller: "console" }]
    });
    expect(handlers.snapshot).toHaveBeenCalledWith(
      [],
      [{ id: "call-8", tool: "files.read", at_ms: 2_000, caller: "console" }]
    );
  });
});
