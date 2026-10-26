// `useRequestGate` is the "newest request wins" rule applied by hand on every page. These tests pin
// the four answers its comment promises: a claimed ticket is current, a newer claim makes it stale,
// each hook counts its own tickets, and unmounting invalidates the last one. A fifth pins that
// claiming a ticket is a ref write, not a render.
import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useRequestGate } from "./requests";

describe("useRequestGate", () => {
  it("answers a freshly claimed ticket as current", () => {
    const { result } = renderHook(() => useRequestGate());

    const ticket = result.current.begin();

    expect(result.current.stale(ticket)).toBe(false);
  });

  it("makes the older ticket stale when a newer one begins", () => {
    const { result } = renderHook(() => useRequestGate());

    const first = result.current.begin();
    const second = result.current.begin();

    expect(result.current.stale(first)).toBe(true);
    expect(result.current.stale(second)).toBe(false);
  });

  it("keeps one gate's begin out of another gate's tickets", () => {
    const { result: one } = renderHook(() => useRequestGate());
    const { result: two } = renderHook(() => useRequestGate());

    const first = one.current.begin();
    const second = one.current.begin();
    expect(one.current.stale(first)).toBe(true);
    expect(one.current.stale(second)).toBe(false);

    const only = two.current.begin();
    expect(two.current.stale(only)).toBe(false);

    // Gate two's begin advanced only gate two's counter. A counter shared between the gates would
    // have made gate one's newest ticket stale here, which is the rule this test exists to pin.
    expect(one.current.stale(second)).toBe(false);
  });

  it("invalidates the last ticket when the page unmounts", () => {
    const { result, unmount } = renderHook(() => useRequestGate());
    const ticket = result.current.begin();
    expect(result.current.stale(ticket)).toBe(false);

    unmount();

    expect(result.current.stale(ticket)).toBe(true);
  });

  it("does not render when a ticket is claimed", () => {
    let renders = 0;
    const { result } = renderHook(() => {
      renders += 1;
      return useRequestGate();
    });
    expect(renders).toBe(1);

    result.current.begin();

    expect(renders).toBe(1);
  });
});
