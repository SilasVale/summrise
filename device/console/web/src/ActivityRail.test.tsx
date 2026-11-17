import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";
import { ActivityRail } from "./ActivityRail";
import type { LiveLine, StartedCall } from "./liveActivity";

const device = "device-1";
const session = "session-1";
const names = new Map([["token-known", "gate-laptop"]]);

/** One `/events` line scoped to this machine and session unless a test says otherwise. */
function line(
  overrides: Partial<LiveLine> & { ts_ms: number; sentence: string }
): LiveLine {
  return { kind: "mcp", status: 200, device_id: device, ...overrides };
}

/** A finished write the rail can draw as a command card. */
function write(
  ts_ms: number,
  input: string,
  caller: "console" | "client" = "console",
  token_id?: string
): LiveLine {
  return line({
    ts_ms,
    tool: "terminal.write",
    caller,
    ...(token_id === undefined ? {} : { token_id }),
    evidence: { session, input },
    sentence: "Typed into a terminal"
  });
}

function started(
  overrides: Partial<StartedCall> & { id: string; tool: string; at_ms: number }
): StartedCall {
  return { device_id: device, caller: "client", ...overrides };
}

function renderRail(props: Partial<ComponentProps<typeof ActivityRail>> = {}) {
  return render(
    <ActivityRail
      session={session}
      deviceId={device}
      lines={[]}
      tokenNames={names}
      {...props}
    />
  );
}

describe("ActivityRail", () => {
  it("draws a finished typed write as a You card with the command in code and the tool as a fact", () => {
    renderRail({
      lines: [
        write(1_000, "npm test -- --run"),
        line({
          ts_ms: 500,
          tool: "terminal.read",
          caller: "console",
          evidence: { session },
          sentence: "Read a terminal"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(within(rail).getAllByText("You")).toHaveLength(2);
    expect(within(rail).getByText("npm test -- --run").tagName).toBe("CODE");
    expect(within(rail).getByText(/terminal\.write/)).toBeInTheDocument();
    // Newest first, and one row for the typed line plus one for the read.
    expect(within(rail).getByText("2 rows")).toBeInTheDocument();
    expect(within(rail).getAllByRole("listitem")).toHaveLength(2);
  });

  it("names a client by its token and an unknown token by its short id", () => {
    renderRail({
      lines: [
        write(2_000, "make", "client", "token-known"),
        write(1_000, "cargo test", "client", "c".repeat(64))
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(within(rail).getByText("gate-laptop")).toBeInTheDocument();
    expect(within(rail).getByText(`${"c".repeat(12)}…`)).toBeInTheDocument();
  });

  it("labels a session event Session and a callerless call Unattributed", () => {
    renderRail({
      lines: [
        line({
          ts_ms: 2_000,
          kind: "session",
          session,
          sentence: "terminal session-1 opened on device-1"
        }),
        line({
          ts_ms: 1_000,
          tool: "terminal.write",
          evidence: { session, input: "whoami" },
          sentence: "Typed into a terminal"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(within(rail).getByText("Session")).toBeInTheDocument();
    // A call's caller is absent in this fixture because only its evidence proves the session.
    expect(within(rail).getByText("Unattributed")).toBeInTheDocument();
    expect(within(rail).getByText("terminal session-1 opened on device-1")).toBeInTheDocument();
  });

  it("shows a running call with its command and no progress, jump or percentage", () => {
    const onJump = vi.fn();
    renderRail({
      onJump,
      running: [
        started({
          id: "call-1",
          tool: "terminal.write",
          at_ms: 1_000,
          token_id: "token-known",
          evidence: { session, input: "npm test" }
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(within(rail).getByText("gate-laptop")).toBeInTheDocument();
    expect(within(rail).getByText("running")).toBeInTheDocument();
    expect(within(rail).getByText("npm test")).toBeInTheDocument();
    expect(within(rail).queryByText(/%/)).not.toBeInTheDocument();
    expect(within(rail).queryByRole("button", { name: "Show in terminal" })).not.toBeInTheDocument();
    expect(onJump).not.toHaveBeenCalled();
  });

  it("keeps another session's and another machine's calls out of this rail", () => {
    renderRail({
      lines: [
        write(3_000, "this session"),
        line({
          ts_ms: 2_000,
          tool: "terminal.write",
          caller: "console",
          evidence: { session: "session-2", input: "another session's" },
          sentence: "Typed into a terminal"
        }),
        line({
          ts_ms: 1_000,
          device_id: "device-2",
          tool: "terminal.write",
          caller: "console",
          evidence: { session, input: "another machine's" },
          sentence: "Typed into a terminal"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(within(rail).getByText("this session")).toBeInTheDocument();
    expect(within(rail).queryByText("another session's")).not.toBeInTheDocument();
    expect(within(rail).queryByText("another machine's")).not.toBeInTheDocument();
  });

  it("says it is connecting before the first snapshot and offline after the connection drops", () => {
    const { rerender } = renderRail({ state: "connecting" });
    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(within(rail).getByText(/Connecting to the console's live activity/)).toBeInTheDocument();
    expect(within(rail).queryByText(/offline/i)).not.toBeInTheDocument();

    rerender(
      <ActivityRail
        session={session}
        deviceId={device}
        lines={[]}
        tokenNames={names}
        state="offline"
      />
    );
    expect(within(rail).getByText(/live activity connection is offline/i)).toBeInTheDocument();
    expect(within(rail).queryByText(/Connecting to the console's live activity/)).not.toBeInTheDocument();
  });

  it("says no activity since this page opened when the live window is empty", () => {
    renderRail({ state: "live" });
    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(
      within(rail).getByText("No activity recorded for this session since this page opened.")
    ).toBeInTheDocument();
  });

  it("discloses the 200-line machine-wide window, the absent per-session history and a lagged count", () => {
    renderRail({ state: "live", lagged: 3 });
    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(within(rail).getByText(/200-line machine-wide window/)).toBeInTheDocument();
    expect(within(rail).getByText(/keeps no per-session history/i)).toBeInTheDocument();
    expect(within(rail).getByText(/dropped 3 live lines/i)).toBeInTheDocument();
  });

  it("counts calls the record cannot place, and never calls them another session's", () => {
    renderRail({
      lines: [
        line({ ts_ms: 2_000, tool: "files.read", caller: "client", sentence: "Read a file" }),
        line({ ts_ms: 1_000, tool: "runs.exec", caller: "console", sentence: "Ran a command" })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(within(rail).getByText(/2 calls on this machine name no session/)).toBeInTheDocument();
    expect(within(rail).getByText(/the record could not place these calls/)).toBeInTheDocument();
    expect(within(rail).getByText(/Snapshot rows .* carry no session or evidence/)).toBeInTheDocument();
    expect(rail.textContent).not.toMatch(/another session/i);
  });

  it("offers Show in terminal only for a finished typed row and passes its text to onJump", () => {
    const onJump = vi.fn();
    renderRail({
      onJump,
      lines: [
        write(2_000, "npm test -- --run"),
        line({
          ts_ms: 1_000,
          tool: "files.read",
          caller: "client",
          token_id: "token-known",
          evidence: { session, path: "/etc/hosts" },
          sentence: "Read a file"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    const jump = within(rail).getByRole("button", { name: "Show in terminal" });
    expect(jump.tagName).toBe("BUTTON");
    expect(jump.tabIndex).toBe(0);
    jump.focus();
    expect(jump).toHaveFocus();

    fireEvent.click(jump);
    expect(onJump).toHaveBeenCalledTimes(1);
    expect(onJump).toHaveBeenCalledWith("npm test -- --run");
  });

  it("keeps a named, keyboard-scrollable region for the rows", () => {
    renderRail({ lines: [write(1_000, "ls")] });
    const rows = screen.getByRole("region", { name: "Activity rows" });
    expect(rows.tabIndex).toBe(0);
    expect(rows).toHaveClass("activity-rail-list");
  });

  it("raises a New activity pill only when the operator has scrolled away and the window grows", async () => {
    const first = write(1_000, "first");
    const { rerender } = renderRail({ lines: [first] });
    const rows = screen.getByRole("region", { name: "Activity rows" });

    // At the top, a new row needs no pill: it is already the first thing the reader sees.
    rerender(
      <ActivityRail
        session={session}
        deviceId={device}
        lines={[write(5_000, "second"), first]}
        tokenNames={names}
      />
    );
    expect(screen.queryByRole("button", { name: "New activity" })).not.toBeInTheDocument();

    // Scrolled away from the newest row, the next arrival is a fact the rail must announce.
    rows.scrollTop = 120;
    fireEvent.scroll(rows);
    rerender(
      <ActivityRail
        session={session}
        deviceId={device}
        lines={[write(9_000, "third"), write(5_000, "second"), first]}
        tokenNames={names}
      />
    );

    const pill = await screen.findByRole("button", { name: "New activity" });
    fireEvent.click(pill);
    expect(rows.scrollTop).toBe(0);
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "New activity" })).not.toBeInTheDocument()
    );
  });

  it("shows a failed row as failed in words, not only in colour", () => {
    renderRail({
      lines: [
        line({
          ts_ms: 1_000,
          tool: "files.write",
          caller: "client",
          ok: false,
          status: 500,
          evidence: { session, path: "/tmp/x" },
          sentence: "files.write failed on device-1"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(within(rail).getByText("failed")).toBeInTheDocument();
    expect(within(rail).getByText("/tmp/x").tagName).toBe("CODE");
  });

  it("makes no control or claim the device cannot honour", () => {
    renderRail({
      onJump: vi.fn(),
      lines: [write(1_000, "ls", "client", "token-known")],
      running: [
        started({
          id: "call-1",
          tool: "terminal.write",
          at_ms: 2_000,
          token_id: "token-known",
          evidence: { session, input: "pwd" }
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(rail.textContent).not.toMatch(/\bAI\b/);
    for (const forbidden of [/take control/i, /approval/i, /goal/i]) {
      expect(within(rail).queryByRole("button", { name: forbidden })).not.toBeInTheDocument();
    }
    // The one control the data supports is the jump to the text this xterm already holds.
    expect(within(rail).getAllByRole("button").map((button) => button.textContent)).toEqual([
      "Show in terminal"
    ]);
  });
});
