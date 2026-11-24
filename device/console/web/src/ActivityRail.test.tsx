import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
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

/** One six-digit colour, resolving the shared stylesheet's canonical aliases. */
function declaredColour(sheet: string, name: string, seen = new Set<string>()): string {
  if (seen.has(name)) throw new Error(`--${name} has a circular colour alias`);
  seen.add(name);
  const match = new RegExp(`--${name}:\\s*(#[0-9a-fA-F]{6}|var\\(--([\\w-]+)\\))\\s*;`).exec(sheet);
  if (match === null) throw new Error(`--${name} has no resolvable colour declaration`);
  return match[2] === undefined
    ? match[1].slice(1).toLowerCase()
    : declaredColour(sheet, match[2], seen);
}

/** WCAG relative luminance of an `rrggbb` colour. */
function luminance(hex: string): number {
  const channels = [0, 2, 4]
    .map((offset) => parseInt(hex.slice(offset, offset + 2), 16) / 255)
    .map((channel) =>
      channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
    );
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
}

/** WCAG contrast between two `rrggbb` colours. */
function contrast(left: string, right: string): number {
  const [lighter, darker] = [luminance(left), luminance(right)].sort((a, b) => b - a);
  return (lighter + 0.05) / (darker + 0.05);
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

  it("draws two commands within the burst as two cards when a control write separates them", () => {
    renderRail({
      onJump: vi.fn(),
      lines: [
        write(3_000, "echo two"),
        line({
          ts_ms: 2_500,
          tool: "terminal.write",
          caller: "console",
          evidence: { session },
          sentence: "Typed into a terminal"
        }),
        write(2_000, "echo one")
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    const commands = within(rail).getAllByText(/^echo /);
    expect(commands.map((node) => node.textContent)).toEqual(["echo two", "echo one"]);
    for (const command of commands) expect(command.tagName).toBe("CODE");
    // Each command keeps its own jump, and the control write earns no card between them.
    expect(within(rail).getAllByRole("button", { name: "Show in terminal" })).toHaveLength(2);
    expect(within(rail).getAllByRole("listitem")).toHaveLength(2);
  });

  it("shows a running call's tool once when its evidence names nothing else", () => {
    renderRail({
      running: [started({ id: "call-1", tool: "files.list", at_ms: 1_000, evidence: { session } })]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    const tool = within(rail).getByText("files.list");
    expect(tool.tagName).toBe("CODE");
    expect(within(rail).getAllByText("files.list")).toHaveLength(1);
    expect(rail.querySelector(".activity-card-facts")).toBeNull();
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
    const withheld = within(rail).getByText(/withheld because they name no session/);
    expect(withheld).toHaveTextContent(
      "2 calls on this machine are withheld because they name no session."
    );
    expect(withheld).toHaveTextContent(
      "Running calls that name no session are counted too; snapshot rows read back from the audit file carry no session or evidence."
    );
    // The sentence counts running calls outside the window as well, so it must not say "in this
    // window": the number is not only the window's lines.
    expect(withheld.textContent).not.toMatch(/in this window/i);
    expect(rail.textContent).not.toMatch(/another session/i);
  });

  it("says one withheld running call the same way, outside the window", () => {
    renderRail({
      running: [started({ id: "call-1", tool: "files.read", at_ms: 1_000 })]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    const withheld = within(rail).getByText(/withheld because it names no session/);
    expect(withheld).toHaveTextContent(
      "1 call on this machine is withheld because it names no session."
    );
    expect(withheld).toHaveTextContent(
      "Running calls that name no session are counted too; snapshot rows read back from the audit file carry no session or evidence."
    );
    expect(withheld.textContent).not.toMatch(/in this window/i);
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

describe("ActivityRail ledger groups", () => {
  /** One row that names the active session, so the rail can place it. */
  function scoped(overrides: Partial<LiveLine> & { ts_ms: number; sentence: string }): LiveLine {
    return line({ ...overrides, evidence: { session, ...overrides.evidence } });
  }

  it("renders a transaction group with its purpose, claim badge and author", () => {
    renderRail({
      lines: [
        scoped({
          ts_ms: 3_000,
          tool: "txn.begin",
          txn: "txn-9c07",
          purpose: "verify the suite",
          caller: "client",
          token_id: "token-known",
          sentence: "Began a transaction"
        }),
        scoped({
          ts_ms: 4_000,
          tool: "runs.exec",
          txn: "txn-9c07",
          caller: "client",
          token_id: "token-known",
          evidence: { command: "npm", args: ["test"] },
          sentence: "Ran `npm test`"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    const group = rail.querySelector(".activity-group");
    expect(group).not.toBeNull();
    expect(within(group as HTMLElement).getByText("verify the suite")).toBeInTheDocument();
    // The purpose is a claim, so the header carries the author's name beside the badge.
    expect(within(group as HTMLElement).getByText("claim")).toBeInTheDocument();
    const head = group?.querySelector(".activity-group-head");
    expect(head).not.toBeNull();
    expect(within(head as HTMLElement).getByText("gate-laptop")).toBeInTheDocument();
    expect(within(group as HTMLElement).getByText("npm test")).toBeInTheDocument();
  });

  it("says an open group has no end recorded, and never closes it", () => {
    renderRail({
      lines: [
        scoped({
          ts_ms: 5_000,
          tool: "txn.begin",
          txn: "txn-1",
          purpose: "fix the parser",
          sentence: "Began a transaction"
        }),
        scoped({
          ts_ms: 4_000,
          tool: "files.write",
          txn: "txn-1",
          evidence: { path: "src/parse.rs" },
          sentence: "Wrote `src/parse.rs`"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(within(rail).getByText(/no end recorded/i)).toBeInTheDocument();
    expect(within(rail).getByText("working")).toBeInTheDocument();
  });

  it("puts a goal divider where the record changed, with the goal's own author", () => {
    renderRail({
      lines: [
        scoped({
          ts_ms: 3_000,
          tool: "terminal.goal",
          goal: { text: "ship the spine" },
          caller: "client",
          token_id: "token-known",
          sentence: "Set the session's goal"
        }),
        scoped({
          ts_ms: 1_000,
          tool: "terminal.goal",
          goal: { text: "read the installer's config" },
          sentence: "Set the session's goal"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(within(rail).getAllByText("goal set")).toHaveLength(2);
    expect(within(rail).getByText("ship the spine")).toBeInTheDocument();
    expect(within(rail).getByText("read the installer's config")).toBeInTheDocument();
    expect(within(rail).getByText("gate-laptop")).toBeInTheDocument();
  });

  it("renders an unknown event neutrally and counts it", () => {
    renderRail({
      lines: [
        scoped({
          ts_ms: 1_000,
          row_kind: "future.thing",
          caller: "client",
          token_id: "token-known",
          sentence: "future.thing on device-1"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(within(rail).getByText(/1 unknown/i)).toBeInTheDocument();
    expect(within(rail).getByText(/unknown event/i)).toBeInTheDocument();
    // Neutral: it is not painted as a failure and carries no invented meaning.
    expect(rail.querySelector(".activity-card.tone-unknown")).not.toBeNull();
    expect(rail.querySelector(".activity-card.tone-failed")).toBeNull();
  });

  it("keeps a declared abort from relabelling the failure the device observed", () => {
    renderRail({
      lines: [
        scoped({
          ts_ms: 5_000,
          tool: "txn.abort",
          txn: "txn-1",
          row_kind: "txn.aborted",
          sentence: "Aborted a transaction",
          receipt: {
            txn: "txn-1",
            state: "aborted",
            reason: "the build was red",
            observed: { calls: 1, failures: 1, refusals: 0, duration_ms: 1_200 }
          }
        }),
        scoped({
          ts_ms: 4_000,
          tool: "runs.exec",
          txn: "txn-1",
          ok: false,
          status: 500,
          evidence: { command: "cargo", args: ["test"] },
          sentence: "`runs.exec` failed"
        }),
        scoped({
          ts_ms: 3_000,
          tool: "txn.begin",
          txn: "txn-1",
          purpose: "fix the parser",
          sentence: "Began a transaction"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    // The group's ending says aborted (in the header and in the receipt's declaration); the call it
    // observed says failed, in its own words.
    const receipt = rail.querySelector(".activity-group-receipt");
    expect(receipt).not.toBeNull();
    expect(within(receipt as HTMLElement).getByText("aborted")).toBeInTheDocument();
    expect(within(rail).getByText("failed")).toBeInTheDocument();
  });

  it("shows the receipt's own totals beside the rows in this window, never instead of them", () => {
    renderRail({
      lines: [
        scoped({
          ts_ms: 5_000,
          tool: "txn.commit",
          txn: "txn-1",
          row_kind: "txn.committed",
          sentence: "Committed a transaction",
          receipt: {
            txn: "txn-1",
            state: "committed",
            observed: { calls: 9, failures: 1, refusals: 2, duration_ms: 192_000 }
          }
        }),
        scoped({
          ts_ms: 4_000,
          tool: "files.write",
          txn: "txn-1",
          evidence: { path: "a.txt" },
          sentence: "Wrote `a.txt`"
        }),
        scoped({
          ts_ms: 3_000,
          tool: "txn.begin",
          txn: "txn-1",
          purpose: "verify the suite",
          sentence: "Began a transaction"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    const group = rail.querySelector(".activity-group") as HTMLElement;
    // The rows in this window stay visible as rows; the receipt's own totals sit beneath them,
    // labelled by which side they come from.
    expect(group.querySelectorAll(".activity-group-calls .activity-card")).toHaveLength(1);
    const parts = group.querySelectorAll(".activity-group-receipt p");
    // Two labelled parts, plus the hint this receipt earns: it committed over a failed call.
    expect(parts).toHaveLength(3);
    // The label carries a separator in the text, not only in the flex gap: a screen reader hears
    // "Declared: committed", never "Declaredcommitted".
    expect(parts[0]).toHaveTextContent("Declared: committed");
    expect(parts[1]).toHaveTextContent("Observed: 9 calls");
    expect(parts[1]).toHaveTextContent(/1 failure/);
    expect(parts[1]).toHaveTextContent(/1 failure/);
    expect(parts[1]).toHaveTextContent(/2 refused/);
    expect(parts[1]).toHaveTextContent(/3m 12s/);
    expect(parts[2]).toHaveTextContent("drift Committed over 1 failed call.");
  });

  it("says where the receipt's declaration and the device's counts disagree", () => {
    renderRail({
      lines: [
        scoped({
          ts_ms: 5_000,
          tool: "txn.commit",
          txn: "txn-1",
          row_kind: "txn.committed",
          sentence: "Committed a transaction",
          receipt: {
            txn: "txn-1",
            state: "committed",
            observed: { calls: 5, failures: 1, refusals: 0, duration_ms: 60_000 }
          }
        }),
        scoped({
          ts_ms: 4_000,
          tool: "files.write",
          txn: "txn-1",
          evidence: { path: "a.txt" },
          sentence: "Wrote `a.txt`"
        }),
        scoped({
          ts_ms: 3_000,
          tool: "txn.begin",
          txn: "txn-1",
          purpose: "verify the suite",
          sentence: "Began a transaction"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    const group = rail.querySelector(".activity-group") as HTMLElement;
    const drift = group.querySelector(".receipt-drift");
    expect(drift, "the receipt foot carries the drift hint").not.toBeNull();
    // The line is labelled as the hint it is, and it states the receipt's own arithmetic: the
    // declared ending (`Committed`) over the calls the device observed (`1 failed call`).
    expect(drift).toHaveTextContent(/^drift\s+Committed over 1 failed call\.$/);
  });

  it("draws no drift line for a clean commit", () => {
    renderRail({
      lines: [
        scoped({
          ts_ms: 5_000,
          tool: "txn.commit",
          txn: "txn-1",
          row_kind: "txn.committed",
          sentence: "Committed a transaction",
          receipt: {
            txn: "txn-1",
            state: "committed",
            observed: { calls: 5, failures: 0, refusals: 0, duration_ms: 60_000 }
          }
        }),
        scoped({
          ts_ms: 3_000,
          tool: "txn.begin",
          txn: "txn-1",
          purpose: "verify the suite",
          sentence: "Began a transaction"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    const group = rail.querySelector(".activity-group") as HTMLElement;
    // Declared and observed agree, so there is nothing to say; the receipt keeps its two parts.
    expect(group.querySelector(".receipt-drift")).toBeNull();
    expect(group.querySelector(".activity-group-receipt")).not.toBeNull();
  });

  it("renders the author's ending when an abort followed clean calls", () => {
    renderRail({
      lines: [
        scoped({
          ts_ms: 5_000,
          tool: "txn.abort",
          txn: "txn-a",
          row_kind: "txn.aborted",
          sentence: "Aborted a transaction",
          receipt: {
            txn: "txn-a",
            state: "aborted",
            observed: { calls: 2, failures: 0, refusals: 0, duration_ms: 30_000 }
          }
        }),
        scoped({
          ts_ms: 3_000,
          tool: "txn.begin",
          txn: "txn-a",
          purpose: "stop before the release",
          sentence: "Began a transaction"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    const group = rail.querySelector(".activity-group") as HTMLElement;
    const drift = group.querySelector(".receipt-drift");
    expect(drift, "an abort over clean calls draws the hint").not.toBeNull();
    expect(drift).toHaveTextContent(
      /^drift\s+Aborted after 2 clean calls — the ending is the author's, not the record's\.$/
    );
  });

  it("renders the no-calls sentence for an empty receipt", () => {
    renderRail({
      lines: [
        scoped({
          ts_ms: 5_000,
          tool: "txn.commit",
          txn: "txn-e",
          row_kind: "txn.committed",
          sentence: "Committed a transaction",
          receipt: {
            txn: "txn-e",
            state: "committed",
            observed: { calls: 0, failures: 0, refusals: 0, duration_ms: 0 }
          }
        }),
        scoped({
          ts_ms: 3_000,
          tool: "txn.begin",
          txn: "txn-e",
          purpose: "an empty transaction",
          sentence: "Began a transaction"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    const group = rail.querySelector(".activity-group") as HTMLElement;
    const drift = group.querySelector(".receipt-drift");
    expect(drift, "an empty receipt draws the hint").not.toBeNull();
    expect(drift).toHaveTextContent(/^drift\s+Ended with no calls recorded\.$/);
  });

  it("never shows a client's purpose as the operator's instruction", () => {
    renderRail({
      lines: [
        scoped({
          ts_ms: 2_000,
          tool: "txn.begin",
          txn: "txn-1",
          purpose: "deploy the client's build",
          caller: "client",
          token_id: "token-known",
          sentence: "Began a transaction"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    expect(within(rail).getByText("deploy the client's build")).toBeInTheDocument();
    expect(within(rail).getByText("gate-laptop")).toBeInTheDocument();
    expect(rail.textContent).not.toMatch(/instruction/i);
  });

  it("draws a refused begin as a refusal, not as a working group", () => {
    renderRail({
      lines: [
        scoped({
          ts_ms: 1_000,
          tool: "txn.begin",
          purpose: "deploy the client's build",
          ok: false,
          status: 403,
          caller: "client",
          token_id: "token-known",
          sentence: "txn.begin was refused on device-1 (tool_not_permitted)"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    // The evidence for a refused begin still carries its purpose; classed as a begin it would draw
    // a header and a working state for a call the machine never ran.
    expect(rail.querySelector(".activity-group")).toBeNull();
    expect(within(rail).queryByText("claim")).not.toBeInTheDocument();
    expect(within(rail).queryByText("working")).not.toBeInTheDocument();
    expect(within(rail).getByText("failed")).toBeInTheDocument();
  });

  it("draws an unknown row inside a group with the spine's class, never as an ok card", () => {
    renderRail({
      lines: [
        scoped({
          ts_ms: 2_000,
          tool: "txn.begin",
          txn: "txn-1",
          purpose: "verify the suite",
          sentence: "Began a transaction"
        }),
        // `goal.set` with no goal is a row the spine cannot place even though its status is 200.
        scoped({
          ts_ms: 3_000,
          tool: "files.read",
          row_kind: "goal.set",
          txn: "txn-1",
          sentence: "files.read ran on device-1"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    const group = rail.querySelector(".activity-group");
    expect(group).not.toBeNull();
    expect(group!.querySelector(".activity-card.tone-unknown")).not.toBeNull();
    expect(group!.querySelector(".activity-card.tone-ok")).toBeNull();
    expect(within(group as HTMLElement).getByText(/unknown event/i)).toBeInTheDocument();
  });

  it("draws the group as a boxed container over a bulletless call list, and a goal as a divider", () => {
    renderRail({
      lines: [
        scoped({
          ts_ms: 1_000,
          tool: "terminal.goal",
          goal: { text: "ship the spine" },
          sentence: "Set the session's goal"
        }),
        scoped({
          ts_ms: 2_000,
          tool: "txn.begin",
          txn: "txn-1",
          purpose: "verify the suite",
          sentence: "Began a transaction"
        }),
        scoped({
          ts_ms: 3_000,
          tool: "files.read",
          txn: "txn-1",
          evidence: { path: "a.txt" },
          sentence: "Read a file"
        })
      ]
    });

    const rail = screen.getByRole("complementary", { name: "Session activity" });
    const group = rail.querySelector("li.activity-group");
    expect(group).not.toBeNull();
    expect(group!.querySelector(".activity-group-head")).not.toBeNull();
    expect(group!.querySelector(".activity-group-foot")).not.toBeNull();
    const calls = group!.querySelector(".activity-group-calls");
    expect(calls).not.toBeNull();
    expect(calls!.tagName).toBe("UL");
    // The body is a plain list of cards; its bullets are removed by the group's own selector, not
    // by an inline style or by making the cards something other than list items.
    for (const child of Array.from(calls!.children)) {
      expect(child.tagName).toBe("LI");
      expect(child).toHaveClass("activity-card");
    }
    const divider = rail.querySelector("li.activity-divider");
    expect(divider).not.toBeNull();
    expect(divider).toHaveAttribute("role", "separator");
    expect(divider!.querySelector(".activity-divider-label")).not.toBeNull();
    expect(divider!.querySelector(".activity-divider-text")).not.toBeNull();
    expect(divider!.querySelector(".activity-divider-by")).not.toBeNull();

    // The classes the rail renders are styled in the shipped sheet, and the nested list declares
    // its own bulletless shape.
    const sheet = readFileSync(path.join(process.cwd(), "src", "app.css"), "utf8");
    expect(sheet).toMatch(/\.activity-group\s*\{[^}]*border: 1px solid var\(--line\)/);
    expect(sheet).toMatch(/\.activity-group-calls\s*\{[^}]*list-style: none/);
    expect(sheet).toMatch(/\.activity-divider\s*\{/);
    expect(sheet).toMatch(/\.activity-rail-unknown\s*\{/);
    expect(sheet).toMatch(/\.activity-card-kind\s*\{/);
    expect(sheet).toMatch(/\.activity-card\.tone-unknown\s*\{/);
    for (const state of ["working", "committed", "aborted", "unrecorded"]) {
      expect(sheet).toMatch(new RegExp(`\\.activity-group\\.state-${state}\\s*\\{`));
    }
  });

  it("the_captured_output_is_paper", () => {
    const sheet = ["tokens.css", "app.css"]
      .map((name) => readFileSync(path.join(process.cwd(), "src", name), "utf8"))
      .join("\n");
    const bg = declaredColour(sheet, "evidence-bg");
    const ink = declaredColour(sheet, "evidence-ink");
    // A captured transcript is terminal output, but it is paper in a one-palette product: the old
    // near-black surface was the last dark rectangle on the light page.
    expect(bg, "the captured output is not the old dark terminal").not.toBe("14161a");
    expect(
      contrast(ink, bg),
      `captured output: #${ink} on #${bg} measures ${contrast(ink, bg).toFixed(2)}:1 (AA needs 4.5)`
    ).toBeGreaterThanOrEqual(4.5);
  });
});
