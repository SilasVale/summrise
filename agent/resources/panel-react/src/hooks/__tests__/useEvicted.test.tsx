// The notice lives for a moment and then leaves, and a second eviction replaces the first.
import { describe, it, expect, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useEvictedNotice } from "../useEvicted";

const frame = (label: string) => ({
  ev: "session-evicted",
  cause: "cap",
  limit: 16,
  sessions: [{ id: `term-${label}`, label, kind: "pty", idle_ms: 1000, reason: "session cap (16) reached" }],
});

describe("useEvictedNotice", () => {
  // EVERY `act` IS `await`ed NOW, AND THAT IS THE PARSE MOVING TO RUST (P2, 2026-09-29) RATHER THAN
  // A TEST BEING MADE TO PASS: the listener dispatches, the seam fetches the wasm, and the notice
  // lands one microtask later — so an `act` that does not await observes the state BEFORE the
  // callback ran. The assertions are unchanged; what changed is when they are allowed to look.
  it("shows the newest eviction and lets it expire", async () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useEvictedNotice(1000));
    expect(result.current).toBeNull();
    await act(async () => {
      window.dispatchEvent(new CustomEvent("summrise-session-evicted", { detail: frame("d1") }));
    });
    expect(result.current?.sessions[0].label).toBe("d1");
    // A second eviction REPLACES the first: two lines about housekeeping is one line too many.
    await act(async () => {
      window.dispatchEvent(new CustomEvent("summrise-session-evicted", { detail: frame("serial:COM4") }));
    });
    expect(result.current?.sessions[0].label).toBe("serial:COM4");
    act(() => {
      vi.advanceTimersByTime(1100);
    });
    expect(result.current).toBeNull();
    vi.useRealTimers();
  });

  it("ignores frames that are not evictions", async () => {
    const { result } = renderHook(() => useEvictedNotice(1000));
    await act(async () => {
      window.dispatchEvent(new CustomEvent("summrise-monitor-change", { detail: { ev: "monitor-change" } }));
    });
    expect(result.current).toBeNull();
  });
});
