import { describe, expect, it } from "vitest";
import {
  openTerminalStream,
  terminalStreamPath,
  type EventSourceLike,
  type StreamHandlers
} from "./terminalStream";

/** A stand-in for the browser's EventSource: the test decides what the stream says. */
class FakeSource implements EventSourceLike {
  readonly url: string;
  onerror: ((event: Event) => void) | null = null;
  closed = false;
  private readonly listeners = new Map<string, ((event: MessageEvent) => void)[]>();

  constructor(url: string) {
    this.url = url;
  }

  addEventListener(type: string, listener: (event: MessageEvent) => void) {
    const list = this.listeners.get(type) ?? [];
    list.push(listener);
    this.listeners.set(type, list);
  }

  close() {
    this.closed = true;
  }

  /** Send one named event, the way the console's stream does. */
  emit(type: string, data: unknown) {
    for (const listener of this.listeners.get(type) ?? []) {
      listener({ data: JSON.stringify(data) } as MessageEvent);
    }
  }

  drop() {
    this.onerror?.(new Event("error"));
  }
}

function recorder() {
  const calls: { kind: string; args: unknown[] }[] = [];
  const handlers: StreamHandlers = {
    output: (data, cursor) => calls.push({ kind: "output", args: [data, cursor] }),
    end: (facts) => calls.push({ kind: "end", args: [facts] }),
    fault: (sentence, status) => calls.push({ kind: "fault", args: [sentence, status] }),
    error: () => calls.push({ kind: "error", args: [] })
  };
  return { calls, handlers };
}

describe("terminalStreamPath", () => {
  it("carries both ids and the cursor", () => {
    expect(terminalStreamPath("ab".repeat(32), "session-3", 12)).toBe(
      `/terminal/${"ab".repeat(32)}/session-3/stream?from=12`
    );
  });

  it("encodes an id that is not a path segment", () => {
    expect(terminalStreamPath("a/b", "c d", 0)).toContain("/terminal/a%2Fb/c%20d/stream");
  });

  it("never asks for a negative cursor", () => {
    expect(terminalStreamPath("d", "s", -4)).toContain("from=0");
  });
});

describe("openTerminalStream", () => {
  it("hands output bytes and the cursor to the caller", () => {
    const { calls, handlers } = recorder();
    let source: FakeSource | null = null;
    openTerminalStream("d", "s", 4, handlers, (url) => (source = new FakeSource(url)));
    source!.emit("output", { data: "hello", cursor: 9 });
    expect(calls).toEqual([{ kind: "output", args: ["hello", 9] }]);
  });

  it("reports the end with the device's own facts", () => {
    const { calls, handlers } = recorder();
    let source: FakeSource | null = null;
    openTerminalStream("d", "s", 0, handlers, (url) => (source = new FakeSource(url)));
    source!.emit("end", { cursor: 30, exit_code: 3, signal: null });
    expect(calls).toEqual([{ kind: "end", args: [{ cursor: 30, exit_code: 3, signal: null }] }]);
    expect(source!.closed).toBe(true);
  });

  it("passes a refused read through as a fault, and closes", () => {
    const { calls, handlers } = recorder();
    let source: FakeSource | null = null;
    openTerminalStream("d", "s", 0, handlers, (url) => (source = new FakeSource(url)));
    source!.emit("fault", { sentence: "no such session", status: 502 });
    expect(calls).toEqual([{ kind: "fault", args: ["no such session", 502] }]);
    expect(source!.closed).toBe(true);
  });

  it("reports a dropped connection once, and closes so nothing reconnects on a stale cursor", () => {
    const { calls, handlers } = recorder();
    let source: FakeSource | null = null;
    openTerminalStream("d", "s", 0, handlers, (url) => (source = new FakeSource(url)));
    source!.drop();
    source!.drop();
    expect(calls).toEqual([{ kind: "error", args: [] }]);
    expect(source!.closed).toBe(true);
  });

  it("ignores a frame that is not JSON rather than throwing inside the listener", () => {
    const { calls, handlers } = recorder();
    let source: FakeSource | null = null;
    openTerminalStream("d", "s", 0, handlers, (url) => (source = new FakeSource(url)));
    source!.emit("output", "not a frame");
    expect(calls).toEqual([]);
  });

  it("returns null where there is no EventSource, so the caller keeps its fallback", () => {
    const { handlers } = recorder();
    expect(openTerminalStream("d", "s", 0, handlers, null)).toBeNull();
  });
});
