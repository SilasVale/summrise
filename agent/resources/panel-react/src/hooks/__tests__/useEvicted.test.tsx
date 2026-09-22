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
  it("shows the newest eviction and lets it expire", () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useEvictedNotice(1000));
    expect(result.current).toBeNull();
    act(() => {
      window.dispatchEvent(new CustomEvent("summrise-session-evicted", { detail: frame("d1") }));
    });
    expect(result.current?.sessions[0].label).toBe("d1");
    // A second eviction REPLACES the first: two lines about housekeeping is one line too many.
    act(() => {
      window.dispatchEvent(new CustomEvent("summrise-session-evicted", { detail: frame("serial:COM4") }));
    });
    expect(result.current?.sessions[0].label).toBe("serial:COM4");
    act(() => {
      vi.advanceTimersByTime(1100);
    });
    expect(result.current).toBeNull();
    vi.useRealTimers();
  });

  it("ignores frames that are not evictions", () => {
    const { result } = renderHook(() => useEvictedNotice(1000));
    act(() => {
      window.dispatchEvent(new CustomEvent("summrise-monitor-change", { detail: { ev: "monitor-change" } }));
    });
    expect(result.current).toBeNull();
  });
});
