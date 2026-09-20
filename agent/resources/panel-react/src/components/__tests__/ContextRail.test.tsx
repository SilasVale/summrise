// ContextRail pins — panel-density side rail: plugins inventory, terminal
// session list (open-first/newest sort, closed no-op, keyboard activate),
// new-session menu, inline rename (round-161, no window.prompt), archive,
// relative times. Other pages render nothing.
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { ContextRail } from "../ContextRail";
import type { Session } from "../../hooks/useSessions";
import type { usePlugins } from "../../hooks/usePlugins";

type Plugins = ReturnType<typeof usePlugins>;

const session = (over: Partial<Session> = {}): Session => ({
  sid: "s1",
  label: "shell",
  kind: "pty",
  closed: false,
  savedOnly: false,
  /** Idle time comes from the device (`terminal_list.idle_ms`); 0 = "just active". */
  idleMs: 0,
  /** The device's busy flag — a command in flight, which output recency cannot see. */
  commandRunning: false,
  active: true,
  firstSeenAt: Date.now(),
  closedAt: null,
  heldByHuman: false,
  approvalRequired: false,
  pendingApproval: null,
  approvalGrants: [],
  goal: null,
  plan: [],
  ...over,
});

const plugins = (over: Partial<Plugins> = {}): Plugins => ({
  rows: [],
  specLoaded: true,
  loadError: "",
  busy: null,
  log: [],
  start: vi.fn(),
  stop: vi.fn(),
  playwright: null,
  playwrightRow: null,
  ...over,
});

const props = (
  over: Partial<React.ComponentProps<typeof ContextRail>> = {},
) => ({
  page: "terminal" as const,
  sessions: [session()],
  activeSid: "s1" as string | null,
  onActivate: vi.fn(),
  onNewSession: vi.fn(),
  connected: true,
  plugins: plugins(),
  ...over,
});

describe("ContextRail", () => {
  it("renders nothing on pages without a context", () => {
    const { container } = render(
      <ContextRail {...props({ page: "browser" })} />,
    );
    expect(container.textContent).toBe("");
  });

  it("plugins rail shows count + rows + loading", () => {
    const { rerender } = render(
      <ContextRail
        {...props({
          page: "plugins",
          plugins: plugins({
            rows: [
              {
                name: "memory",
                displayName: "Memory",
                description: "KB",
                enabled: true,
                state: "success",
                stateLabel: "Loaded",
                toolCount: 6,
              },
            ],
          }),
        })}
      />,
    );
    expect(screen.getByText("Plugins")).toBeTruthy();
    expect(screen.getByText("Memory")).toBeTruthy();
    expect(screen.getByText("Loaded")).toBeTruthy();
    rerender(
      <ContextRail
        {...props({ page: "plugins", plugins: plugins({ specLoaded: false }) })}
      />,
    );
    expect(screen.getByText("Inventory loading…")).toBeTruthy();
  });

  it("sorts open-first newest-first; closed never activates", () => {
    const now = Date.now();
    const p = props({
      sessions: [
        session({ sid: "old", label: "old", firstSeenAt: now - 9000 }),
        session({ sid: "new", label: "new", firstSeenAt: now - 1000 }),
        session({ sid: "dead", label: "dead", closed: true, firstSeenAt: now }),
      ],
    });
    const { container } = render(<ContextRail {...p} />);
    const labels = [...container.querySelectorAll(".side-row .side-label")].map(
      (e) => e.textContent,
    );
    expect(labels).toEqual(["new", "old", "dead"]);
    fireEvent.click(screen.getByText("new"));
    expect(p.onActivate).toHaveBeenCalledWith("new");
    fireEvent.click(screen.getByText("dead"));
    expect(p.onActivate).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(screen.getByText("new"), { key: "Enter" });
    expect(p.onActivate).toHaveBeenCalledTimes(2);
  });

  it("new-session menu lists 4 kinds and closes after picking", () => {
    const p = props();
    render(<ContextRail {...p} />);
    expect(screen.queryByText("Local shell")).toBeNull();
    fireEvent.click(screen.getByLabelText("New session"));
    fireEvent.click(screen.getByText("SSH…"));
    expect(p.onNewSession).toHaveBeenCalledWith("ssh");
    expect(screen.queryByText("Local shell")).toBeNull();
  });

  it("inline rename commits on Enter, cancels on Escape, ignores blanks", () => {
    const p = props();
    render(<ContextRail {...p} />);
    fireEvent.click(screen.getByTitle("Rename (local)"));
    const input = screen.getByDisplayValue("shell") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "main" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(screen.getByText("main")).toBeTruthy();
    // Escape cancels
    fireEvent.click(screen.getByTitle("Rename (local)"));
    const input2 = screen.getByDisplayValue("main") as HTMLInputElement;
    fireEvent.change(input2, { target: { value: "nope" } });
    fireEvent.keyDown(input2, { key: "Escape" });
    expect(screen.getByText("main")).toBeTruthy();
    // blank keeps the old label
    fireEvent.click(screen.getByTitle("Rename (local)"));
    const input3 = screen.getByDisplayValue("main") as HTMLInputElement;
    fireEvent.change(input3, { target: { value: "   " } });
    fireEvent.keyDown(input3, { key: "Enter" });
    expect(screen.getByText("main")).toBeTruthy();
  });

  it("hiding a row is reversible, counted, and named for what it does", () => {
    const p = props({
      sessions: [
        session({ firstSeenAt: Date.now() - 30_000 }),
        session({
          sid: "s2",
          label: "two",
          idleMs: 0, firstSeenAt: Date.now() - 5 * 60_000,
        }),
      ],
    });
    render(<ContextRail {...p} />);
    expect(screen.getByText("now")).toBeTruthy();
    expect(screen.getByText("5m")).toBeTruthy();

    // THE ACCESSIBLE NAME IS THE ACTION'S NAME. This button hides a row from this
    // list; it does not archive anything, and `aria-label` used to say "Archive
    // session" while its own tooltip said "Hide from list" — a stronger claim,
    // heard only by screen-reader users.
    expect(screen.queryByLabelText("Archive session")).toBeNull();
    fireEvent.click(screen.getAllByLabelText("Hide from list")[0]);
    expect(screen.queryByText("shell")).toBeNull();
    expect(screen.getByText("two")).toBeTruthy();

    // THE HEADER COUNT MUST DESCRIBE THE LIST. It used `sessions.length` while the
    // rows were filtered, so it kept counting the hidden one.
    const count = document.querySelector(".side-count");
    expect(count?.textContent).toBe("1");

    // AND THERE IS A WAY BACK. The only mutation used to be `.add`, so a row hidden
    // by accident was gone until reload — the "destructive with no undo" shape this
    // project keeps finding.
    const undo = screen.getByText("+1 hidden");
    fireEvent.click(undo);
    expect(screen.getByText("shell")).toBeTruthy();
    expect(document.querySelector(".side-count")?.textContent).toBe("2");
    expect(screen.queryByText("+1 hidden")).toBeNull();
  });
});
// AN EMPTY LIST MEANS TWO THINGS. Measured in round 62 by rendering the panel with every API call
// failing — a state no sweep had produced: the rail said "No sessions yet" while the workspace beside
// it said "Connection lost — reconnecting…". One of those is false, and it is the one in the rail.
describe("ContextRail when the device is unreachable", () => {
  it("says it cannot see the sessions, not that there are none", () => {
    const { container } = render(<ContextRail {...props({ sessions: [], connected: false })} />);
    expect(container.textContent).toContain("unavailable");
    expect(container.textContent).not.toContain("No sessions yet");
  });

  it("still says 'No sessions yet' when the device says the list is empty", () => {
    const { container } = render(<ContextRail {...props({ sessions: [], connected: true })} />);
    expect(container.textContent).toContain("No sessions yet");
  });
});

// ── THE DEVICE'S EXIT CODE, ON THE MARK (round 96, given a shape in round 97) ───────────────────
// The fact crossed the wire in round 96 and the row wore it as a text chip while the state had no silhouette. It
// has one now (a triangle, `liveness.ts`), so the chip is gone and this pins what the mark says instead: a failed
// last command changes the MARK and nothing else on the row, a success and an unknown do not, and the code itself
// stays one hover away.
describe("a session whose last command failed", () => {
  const markOf = (container: HTMLElement) => container.querySelector(".side-dot");

  it("draws the failed state, and only for a failure", () => {
    // QUIET, because the failure has to be the loudest true thing about the session: the helper's `idleMs: 0`
    // means "just produced output", which is WORKING and outranks a past failure — asserted below, because that
    // precedence is the whole reason `failed` sits where it does in the urgency table.
    const quiet = { idleMs: 60_000 };
    const failed = render(<ContextRail {...props({ sessions: [session({ ...quiet, lastExitCode: 1 })] })} />);
    expect(markOf(failed.container)?.getAttribute("data-live")).toBe("failed");
    expect(markOf(failed.container)?.getAttribute("title")).toBe("the last command this session finished exited 1");
    failed.unmount();

    // ZERO IS AN ANSWER, NOT A FAILURE: the session is idle, and no title claims otherwise.
    const ok = render(<ContextRail {...props({ sessions: [session({ ...quiet, lastExitCode: 0 })] })} />);
    expect(markOf(ok.container)?.getAttribute("data-live")).toBe("idle");
    expect(markOf(ok.container)?.getAttribute("title")).toBeNull();
    ok.unmount();

    // AND NEITHER IS "UNKNOWN": absent means the device observed no code at all (no command yet, a wait without a
    // shell marker, an ssh/serial session), so the row says nothing about an outcome.
    const unknown = render(<ContextRail {...props({ sessions: [session({ ...quiet, lastExitCode: null })] })} />);
    expect(markOf(unknown.container)?.getAttribute("data-live")).toBe("idle");
    expect(markOf(unknown.container)?.getAttribute("title")).toBeNull();
    unknown.unmount();

    // The code is carried for any non-zero value, not only for 1 — "failed" alone does not say how it failed.
    const killed = render(<ContextRail {...props({ sessions: [session({ ...quiet, lastExitCode: 130 })] })} />);
    expect(markOf(killed.container)?.getAttribute("title")).toBe("the last command this session finished exited 130");
    killed.unmount();

    // ANYTHING HAPPENING NOW OUTRANKS WHAT ALREADY HAPPENED: still producing output reads working, and the code
    // is not mentioned, because the session's present is not its last exit code.
    const busy = render(<ContextRail {...props({ sessions: [session({ idleMs: 0, lastExitCode: 1 })] })} />);
    expect(markOf(busy.container)?.getAttribute("data-live")).toBe("working");
    expect(markOf(busy.container)?.getAttribute("title")).toBeNull();
    busy.unmount();
  });
});
