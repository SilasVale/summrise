// TabBar pins — session tabs: closed tabs never activate (round-161 honest
// label, round-113 no-op), export doesn't activate (stopPropagation),
// two-step close confirm (P1-5, memory_delete pattern), view switch only
// while a session is active.
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { TabBar } from "../TabBar";
import type { Session } from "../../hooks/useSessions";

const session = (over: Partial<Session> = {}): Session => ({
  sid: "s1",
  label: "shell",
  kind: "pty",
  closed: false,
  savedOnly: false,
  active: true,
  // A QUIET SESSION BY DEFAULT, because `idleMs: 0` means "this session produced output within the window" — which
  // is `working`, correctly, once the model reads the device's own per-session fact. The old default silently
  // claimed every fixture session was active (round 9 of the standing goal).
  idleMs: 60_000, commandRunning: false, firstSeenAt: Date.now(),
  closedAt: null,
  heldByHuman: false,
  approvalRequired: false,
  pendingApproval: null,
  approvalGrants: [],
  goal: null,
  plan: [],
  ...over,
});

const props = (over: Partial<React.ComponentProps<typeof TabBar>> = {}) => ({
  sessions: [session()],
  activeSid: "s1" as string | null,
  onActivate: vi.fn(),
  onClose: vi.fn(),
  onExport: vi.fn(),
  view: "terminal" as const,
  onViewChange: vi.fn(),
  ...over,
});

describe("TabBar", () => {
  it("clicking a live tab activates it; closed tabs are a silent no-op", () => {
    const p = props({
      sessions: [
        session(),
        session({ sid: "s2", label: "dead", closed: true }),
      ],
    });
    render(<TabBar {...p} />);
    fireEvent.click(screen.getByText("shell"));
    expect(p.onActivate).toHaveBeenCalledWith("s1");
    fireEvent.click(screen.getByText("dead"));
    expect(p.onActivate).toHaveBeenCalledTimes(1);
    // The label names where the trail ACTUALLY is. It used to promise
    // "Trajectory/Logs" — there is no Logs view, and Trajectory shows the
    // active session, so the promise could not be kept (ArchivePage is the
    // surface that reads this device's recorded sessions).
    expect(
      screen.getByTitle("dead — closed (its recorded trail is in Archive)"),
    ).toBeTruthy();
  });

  it("export does not activate the tab", () => {
    const p = props();
    render(<TabBar {...p} />);
    fireEvent.click(screen.getByTitle("Export this session log"));
    expect(p.onExport).toHaveBeenCalledWith("s1");
    expect(p.onActivate).not.toHaveBeenCalled();
  });

  it("two-step close: arm → Close executes, Cancel disarms", () => {
    const p = props();
    render(<TabBar {...p} />);
    fireEvent.click(screen.getByTitle("Close session"));
    expect(p.onClose).not.toHaveBeenCalled();
    expect(screen.getByText("close?")).toBeTruthy();
    fireEvent.click(screen.getByText("Cancel"));
    expect(screen.queryByText("close?")).toBeNull();
    expect(p.onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTitle("Close session"));
    fireEvent.click(screen.getByText("Close"));
    expect(p.onClose).toHaveBeenCalledWith("s1");
  });

  it("savedOnly and closed sessions show no close affordance", () => {
    const p = props({
      sessions: [
        session({ sid: "s9", label: "saved", savedOnly: true }),
        session({ sid: "s8", label: "gone", closed: true }),
      ],
      activeSid: null,
    });
    render(<TabBar {...p} />);
    expect(screen.queryByTitle("Close session")).toBeNull();
  });

  it("does NOT carry the view switch — it moved to the session control bar", () => {
    // ROUND 169'S DESIGN DECISION, PINNED HERE SO IT CANNOT CREEP BACK. The switch used to render in
    // this strip, competing for the width the tabs need: ten of sixteen tabs rendered as `pws…` on the
    // live device. It now lives in the control bar TerminalWorkspace builds for both densities. If
    // someone adds it back to the row, this fails and points at why.
    render(<TabBar {...props()} />);
    expect(screen.queryByText("Timeline")).toBeNull();
    expect(screen.queryByText("Steps")).toBeNull();
  });
});

describe("TabBar — a question waiting for a person", () => {
  const question = {
    id: "g1",
    command: "reload",
    expiresAtMs: Date.now() + 60_000,
  };

  it("marks the session that is holding a question, with a shape AND a word", () => {
    const p = props({
      sessions: [
        session(),
        session({ sid: "s2", label: "gated", pendingApproval: question }),
      ],
    });
    const { container } = render(<TabBar {...p} />);
    // THE DOT ITSELF CARRIES THE STATE NOW. A waiting session used to draw a SECOND element
    // (.tab-wait) beside the lane dot, because the dot could only carry a lane colour — so with
    // sixteen tabs the only way to find the one holding a question was to read its aria-label.
    // There is one mark per session, and its `data-live` is the state; the lane colour rides along.
    expect(container.querySelectorAll('.tab-dot[data-live="waiting"]')).toHaveLength(1);
    const tab = screen.getByTitle("gated — waiting for your approval");
    expect(tab.querySelector('.tab-dot[data-live="waiting"]')).toBeTruthy();
    // every other tab is idle, and none of them is waiting
    expect(container.querySelectorAll('.tab-dot[data-live="idle"]')).toHaveLength(1);
    // ...and the word, for anyone who cannot see the mark.
    expect(tab.getAttribute("aria-label")).toBe(
      "gated — waiting for your approval",
    );
    // The unmarked session keeps its ordinary title.
    expect(screen.getByTitle("s1")).toBeTruthy();
  });

  it("does NOT mark a session that is merely ARMED", () => {
    // Keying the badge off `approvalRequired` would make every armed session
    // shout forever, which is how a badge becomes wallpaper.
    const { container } = render(
      <TabBar
        {...props({
          sessions: [
            session({ sid: "s3", label: "armed", approvalRequired: true }),
          ],
        })}
      />,
    );
    expect(container.querySelector('.tab-dot[data-live="waiting"]')).toBeNull();
    expect(screen.queryByTitle("armed — waiting for your approval")).toBeNull();
    expect(screen.getByTitle("s3")).toBeTruthy();
    expect(
      screen.queryByLabelText("armed — waiting for your approval"),
    ).toBeNull();
  });

  it("never marks a CLOSED tombstone, even if it still carries a question", () => {
    const { container } = render(
      <TabBar
        {...props({
          sessions: [
            session({
              sid: "s9",
              label: "gone",
              closed: true,
              pendingApproval: question,
            }),
          ],
          activeSid: null,
        })}
      />,
    );
    // A CLOSED tombstone is `off` in the model, not `waiting`: nothing can be answered on it even
    // though the row still carries the question.
    expect(container.querySelector('.tab-dot[data-live="waiting"]')).toBeNull();
    expect(container.querySelector('.tab-dot[data-live="off"]')).toBeTruthy();
    expect(
      screen.getByTitle("gone — closed (its recorded trail is in Archive)"),
    ).toBeTruthy();
  });

  it("wears the halo on the session that is producing output, and only that one", () => {
    // THE FACT THE DEVICE ALREADY HAD. `idle_ms` is the agent's own `last_output.elapsed()` per session; the panel
    // typed it and ignored it, passing `active: false` everywhere because the only signal in use was DEVICE-wide.
    // Two sessions: the one that just printed is `working`, the quiet one is not.
    const { container } = render(
      <TabBar {...props({ sessions: [session({ sid: "s1", label: "busy", idleMs: 400 }), session({ sid: "s2", label: "quiet", idleMs: 90_000 })] })} />,
    );
    expect(container.querySelectorAll('.tab-dot[data-live="working"]')).toHaveLength(1);
    expect(container.querySelectorAll('.tab-dot[data-live="idle"]')).toHaveLength(1);
    // the title of a plain tab is its SID (only a closed or waiting tab is titled by its label)
    expect(screen.getByTitle("s1").querySelector('.tab-dot')!.getAttribute("data-live")).toBe("working");
    expect(screen.getByTitle("s2").querySelector('.tab-dot')!.getAttribute("data-live")).toBe("idle");
  });

  it("shows no COUNT — the mark is a state, not a tally", () => {
    // The old mark was an empty element that could have grown a number. The state model has no
    // count in it at all: one session holds at most one question, and the strip shows the session
    // rather than a total. The device-wide total lives in the status bar and the tab title.
    const { container } = render(
      <TabBar
        {...props({
          sessions: [
            session({ sid: "s2", label: "gated", pendingApproval: question }),
          ],
        })}
      />,
    );
    const dot = container.querySelector('.tab-dot[data-live="waiting"]')!;
    expect(dot.textContent).toBe("");
  });
});
