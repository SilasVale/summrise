// ONE CLOCK, AND IT STOPS WHEN NOTHING IS MOVING (round 62).
//
// `useNow` exists because five components rendered `Date.now() - startedAt` and re-rendered only when their props
// changed — so a running command's duration froze whenever it produced no output (measured in CommandCard.test.tsx:
// "517ms" stayed "517ms" across five seconds). The fix is one shared clock, and the reason it takes an `active` flag
// is the other half of the objective: a panel that repaints every second with nothing running is the idle repaint
// this codebase measures elsewhere. Both halves are pinned here, and the idle half is measured as "no timer exists"
// rather than as "the value did not change", because a timer that fires and sets the same value is still a repaint.
import { describe, it, expect, vi, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useNow } from "../useNow";

afterEach(() => {
  vi.useRealTimers();
});

describe("useNow", () => {
  it("runs a clock while active and advances with it", () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useNow(true, 1000));
    const first = result.current;
    act(() => {
      vi.advanceTimersByTime(3000);
    });
    expect(result.current).toBeGreaterThan(first);
    expect(vi.getTimerCount()).toBeGreaterThan(0);
  });

  it("keeps NO timer at all while inactive — an idle panel must not repaint", () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useNow(false, 1000));
    const first = result.current;
    act(() => {
      vi.advanceTimersByTime(10_000);
    });
    expect(result.current).toBe(first);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("starts ticking when the thing it measures starts, without waiting a full interval", () => {
    vi.useFakeTimers();
    const { result, rerender } = renderHook(({ active }) => useNow(active, 1000), {
      initialProps: { active: false },
    });
    const idle = result.current;
    act(() => {
      vi.advanceTimersByTime(7500);
    });
    rerender({ active: true });
    // the value is read fresh on activation, so the first paint after "running" is not up to a second stale
    expect(result.current).toBeGreaterThan(idle);
    expect(vi.getTimerCount()).toBe(1);
  });
});