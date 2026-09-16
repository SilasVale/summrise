// Coverage audit row 18: useSSE had no test file. The connected path opens
// an SSE stream + reconnect loop (complex to mock deterministically); the
// not-connected guard is the cheap, high-value contract — the panel must
// NOT open a stream (or start a reconnect loop) while disconnected.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { useSSE } from "../useSSE";
import { initTransport } from "../../lib/api";

beforeEach(() => { localStorage.clear(); vi.useFakeTimers(); });
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

describe("useSSE", () => {
  it("returns connecting and opens nothing when the panel is not connected", () => {
    const fetchMock = vi.fn();
    globalThis.fetch = fetchMock;
    const writeCallbacks = { current: new Map() };
    const getLiveSidsRef = { current: () => [] };
    const { result } = renderHook(() => useSSE(false, writeCallbacks, getLiveSidsRef));
    expect(result.current).toBe("connecting");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("builds the stream URL + Bearer from the transport singleton (P2-1b)", async () => {
    vi.useRealTimers();
    initTransport("dev1.example.com", "tok-1", () => {});
    const fetchMock = vi.fn(async () => new Response("x", { status: 401 }));
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    const writeCallbacks = { current: new Map() };
    const getLiveSidsRef = { current: () => [] };
    const { unmount } = renderHook(() => useSSE(true, writeCallbacks, getLiveSidsRef));
    await waitFor(() => expect(fetchMock).toHaveBeenCalled(), { timeout: 3000 });
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, { headers?: Record<string, string> }];
    expect(String(url)).toContain("dev1.example.com/api/events/term");
    expect(init?.headers?.authorization).toBe("Bearer tok-1");
    unmount();
  });
});

// ── THE FRAME PARSER, against the shapes the device really sends ─────────────────────────────────
//
// The tests above cover the URL and the disconnected guard; the parser had none, and it is where the
// subtlety lives: absolute offsets and dedup against the sync read. The frames below are copied from
// the device's own builders in `agent/src/web/sse.rs`, which is why one of them is a bare boolean.
//
// WHAT THESE DO AND DO NOT PIN, stated because I twice wrote a title claiming more than the assertions
// delivered. They pin the PANEL's handling of those shapes — a regression in the parser fails here.
// They do NOT pin the device to them: the shapes are written out in the test, not read from the Rust,
// so a rename on the device side would leave these passing and the panel silently ignoring frames. The
// NAMES have a cross-boundary contract (`src/lib/eventVocabulary.test.ts`); the PAYLOAD KEYS do not,
// and that gap is real: the lag recovery's own effect also cannot be observed here — the sync path
// calls `terminal_read` regardless, so an assertion on it would pass with the recovery deleted, which
// I checked by deleting it.
describe("useSSE's frame handling", () => {
  /** A stream that emits the given SSE frames and then stays open. */
  function streamOf(frames: string[]): typeof fetch {
    const body = frames.join("");
    return (async () =>
      new Response(
        new ReadableStream({
          start(controller) {
            controller.enqueue(new TextEncoder().encode(body));
            // Left open on purpose: closing would trip the reconnect path.
          },
        }),
        { status: 200, headers: { "content-type": "text/event-stream" } },
      )) as unknown as typeof fetch;
  }

  function setup(writeCallbacks: { current: Map<string, unknown> }) {
    initTransport("dev1.example.com", "tok-1", () => {});
    const getLiveSidsRef = { current: () => ["term-a"] };
    return renderHook(() => useSSE(true, writeCallbacks as never, getLiveSidsRef));
  }

  it("announces a control frame under its derived name", async () => {
    vi.useRealTimers();
    globalThis.fetch = streamOf(['data: {"v":1,"ev":"sessions-changed"}\n\n']);
    const seen: string[] = [];
    const onEv = (e: Event) => seen.push((e as CustomEvent).detail.ev);
    window.addEventListener("vale-sessions-changed", onEv);
    const { unmount } = setup({ current: new Map() });
    await waitFor(() => expect(seen).toContain("sessions-changed"), { timeout: 3000 });
    window.removeEventListener("vale-sessions-changed", onEv);
    unmount();
  });

  it("writes output bytes at the ABSOLUTE offset the device attaches", async () => {
    vi.useRealTimers();
    // {"start":4,"data":[98,99]} — bytes 4 and 5 of the session. The offset is the contract that
    // keeps a reconnect from duplicating output, which is what the sync-read dedup compares against.
    globalThis.fetch = streamOf(['data: {"v":1,"session_id":"term-a","start":4,"data":[98,99]}\n\n']);
    const write = vi.fn();
    const cb = { getRendered: () => 4, write, setRendered: vi.fn() };
    const { unmount } = setup({ current: new Map([["term-a", cb]]) });
    await waitFor(() => expect(write).toHaveBeenCalled(), { timeout: 3000 });
    expect(write.mock.calls[0][1], "the frame's own start offset travels with the bytes").toBe(4);
    unmount();
  });

  it("survives the device's lagged frame — which carries no session_id — and lands the next one", async () => {
    vi.useRealTimers();
    // EXACTLY what sse.rs sends when the broadcast drops frames for a slow subscriber. It has no
    // session_id — the broadcast is cross-session — and the panel responds by marking EVERY session
    // for a gap backfill, then uses the next frame's `start` as the true lower bound of what was lost.
    // The device's comment called this "ignored client-side"; this is what actually happens.
    globalThis.fetch = streamOf([
      'data: {"v":1,"lagged":true}\n\n',
      'data: {"v":1,"session_id":"term-a","start":10,"data":[120]}\n\n',
    ]);
    const write = vi.fn();
    const cb = { getRendered: () => 0, write, setRendered: vi.fn() };
    const { unmount } = setup({ current: new Map([["term-a", cb]]) });
    await waitFor(() => expect(write).toHaveBeenCalled(), { timeout: 3000 });
    expect(write.mock.calls[0][1]).toBe(10);
    unmount();
  });
});
