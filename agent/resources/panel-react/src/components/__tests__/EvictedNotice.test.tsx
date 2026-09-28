// Nothing happened → nothing is drawn (not an empty container), and the line carries a dismiss.
import { describe, it, expect, vi } from "vitest";
import { render, fireEvent } from "@testing-library/react";
import { EvictedNotice } from "../EvictedNotice";
import { parseEvicted } from "../../lib/evicted";

// TOP-LEVEL `await`, because the parse is RUST NOW (P2, 2026-09-29) and the wasm is fetched at the
// first call. Vitest compiles these files as ESM, so a module-level await is available — and the
// alternative (a hand-written literal) would have made this file stop testing the PARSER'S output,
// which is part of what it was doing. The same object, through the same parser, one tick later.
const notice = (await parseEvicted({
  ev: "session-evicted",
  cause: "cap",
  limit: 16,
  sessions: [{ id: "term-a-1", label: "d1", kind: "pty", idle_ms: 1000, reason: "session cap (16) reached" }],
}))!;

describe("EvictedNotice", () => {
  it("draws nothing when the device has taken nothing", () => {
    const { container } = render(<EvictedNotice notice={null} />);
    expect(container.firstChild).toBeNull();
  });

  it("states what was closed and by which rule, and can be dismissed", () => {
    const onDismiss = vi.fn();
    const { container, getByLabelText } = render(<EvictedNotice notice={notice} onDismiss={onDismiss} />);
    expect(container.firstChild).not.toBeNull();
    expect(container.textContent).toContain("d1");
    expect(container.textContent).toContain("16-session cap");
    // Housekeeping is not an alarm: it must not claim a live region that interrupts.
    expect(container.firstChild).toHaveProperty("getAttribute", expect.any(Function));
    expect((container.firstChild as HTMLElement).getAttribute("role")).toBe("status");
    fireEvent.click(getByLabelText("Dismiss"));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});
