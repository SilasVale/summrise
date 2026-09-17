// The offer's behaviour: nothing when there is nothing, a confirmation before anything is closed,
// and exactly the candidates closed when it is.
import { describe, it, expect, vi } from "vitest";
import { render, fireEvent } from "@testing-library/react";
import { IdleSessionsBar } from "../IdleSessionsBar";
import type { Session } from "../../hooks/useSessions";

const idle = (sid: string, label: string): Session =>
  ({
    sid, label, kind: "pty", closed: false, savedOnly: false, active: false,
    idleMs: 3 * 3600_000, commandRunning: false, firstSeenAt: 0, closedAt: null, heldByHuman: false,
    approvalRequired: false, pendingApproval: null, approvalGrants: [], goal: null, plan: [],
  }) as Session;

describe("IdleSessionsBar", () => {
  it("draws nothing when nothing is idle", () => {
    const { container } = render(<IdleSessionsBar candidates={[]} onClose={vi.fn()} />);
    expect(container.firstChild).toBeNull();
  });

  it("asks before closing, and closes every candidate when told to", () => {
    const onClose = vi.fn();
    const { getByText, container } = render(
      <IdleSessionsBar candidates={[idle("a", "d1"), idle("b", "serial:COM4")]} onClose={onClose} />,
    );
    // The offer states the facts; nothing is closed until the second click.
    expect(container.textContent).toContain("2 sessions");
    fireEvent.click(getByText("Close them"));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(getByText("Close 2"));
    expect(onClose.mock.calls.map((c) => c[0])).toEqual(["a", "b"]);
  });

  it("hides when asked to hide, and comes back when the list changes", () => {
    // The dismiss button's label promises "hide until the list changes"; before round 42 its handler
    // only cancelled the confirmation, so the bar stayed exactly where it was.
    const onClose = vi.fn();
    const { container, getByLabelText, rerender } = render(
      <IdleSessionsBar candidates={[idle("a", "d1")]} onClose={onClose} />,
    );
    fireEvent.click(getByLabelText("Hide until the list changes"));
    expect(container.firstChild).toBeNull();
    // The SAME set stays hidden across re-renders…
    rerender(<IdleSessionsBar candidates={[idle("a", "d1")]} onClose={onClose} />);
    expect(container.firstChild).toBeNull();
    // …and a new idle session is new information, so the offer returns.
    rerender(<IdleSessionsBar candidates={[idle("a", "d1"), idle("b", "pwsh")]} onClose={onClose} />);
    expect(container.firstChild).not.toBeNull();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("lets the operator change their mind", () => {
    const onClose = vi.fn();
    const { getByText } = render(<IdleSessionsBar candidates={[idle("a", "d1")]} onClose={onClose} />);
    fireEvent.click(getByText("Close them"));
    fireEvent.click(getByText("Keep them"));
    expect(onClose).not.toHaveBeenCalled();
    // …and the offer is still there afterwards, unclosed.
    expect(getByText("Close them")).toBeTruthy();
  });
});
