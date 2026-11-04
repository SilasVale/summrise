import { describe, expect, it, vi } from "vitest";
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
    expect(handlers.snapshot).toHaveBeenCalledWith([
      { ts_ms: 1, kind: "mcp", status: 200, ok: true, sentence: "Read `a.txt`" },
      { ts_ms: 2, kind: "mcp", status: 200, ok: true, sentence: "Issued a grant" }
    ]);
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

  it("reports lag and a dropped connection", () => {
    const source = new FakeSource("/events");
    const handlers = { snapshot: vi.fn(), activity: vi.fn(), lagged: vi.fn(), error: vi.fn() };
    openLiveActivity(handlers, () => source);
    source.emit("lagged", {});
    source.fail();
    expect(handlers.lagged).toHaveBeenCalledOnce();
    expect(handlers.error).toHaveBeenCalledOnce();
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
});
