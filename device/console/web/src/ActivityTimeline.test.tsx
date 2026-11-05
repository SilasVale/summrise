import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AuditLine, Device, TokenMeta } from "./api";
import { ActivityTimeline } from "./ActivityTimeline";

// The timeline is the proof, not the summary: a row that opens must show the call as it was sent and
// what came back. A row fetched from the audit file has no such body and must say why rather than
// draw an empty panel — the two cases look alike in the list and differ exactly here.

function line(overrides: Partial<AuditLine> = {}): AuditLine {
  return {
    ts_ms: Date.UTC(2026, 10, 5, 9, 0, 0),
    kind: "mcp",
    device_id: null,
    tool: null,
    detail: null,
    status: 200,
    ...overrides
  };
}

const alpha = "device-alpha-0123456789ab";
const beta = "device-beta-0123456789ab";

const devices: Device[] = [
  { id: alpha, name: "Alpha", online: true, version: "1.0.0", last_seen_ms: 1 },
  { id: beta, name: "Beta", online: false, version: null, last_seen_ms: null }
];

const fromAlpha = line({
  ts_ms: Date.UTC(2026, 10, 5, 9, 0, 0),
  device_id: alpha,
  sentence: "Read notes.txt",
  tool: "files.read",
  detail: "notes.txt"
});

const fromBeta = line({
  ts_ms: Date.UTC(2026, 10, 5, 9, 1, 0),
  device_id: beta,
  sentence: "Ran a command",
  tool: "runs.exec",
  detail: "exit 0"
});

const tokenA = "a".repeat(64);
const tokens: TokenMeta[] = [
  {
    id: tokenA,
    name: "laptop-dsh",
    device_id: alpha,
    tools: ["files.read"],
    allow_destructive: false,
    created_at_ms: 1
  }
];

/** The evidence panel, which exists only while the row that carries it is open. */
function evidencePanel(): HTMLElement {
  const panel = document.querySelector<HTMLElement>(".evidence");
  expect(panel).not.toBeNull();
  return panel as HTMLElement;
}

/** Open the only closed row on the page; the row itself is the control. */
function openRow() {
  fireEvent.click(screen.getByRole("button", { expanded: false }));
}

describe("ActivityTimeline stretches", () => {
  it("says what the stretch that just ended held", () => {
    // Three calls inside a minute, then a gap of 61 s, then one more: the boundary is derived from the
    // same rule that draws the hairline, and the label reports the rows above it, nothing invented.
    // Newest first, as the page hands them over: one stretch of three calls a minute wide, then a
    // 61 s gap, then a lone call.
    const start = Date.UTC(2026, 10, 5, 9, 0, 0);
    const lines = [
      line({ ts_ms: start + 60_000, device_id: alpha, sentence: "Third", tool: "files.read" }),
      line({ ts_ms: start + 30_000, device_id: alpha, sentence: "Second", tool: "files.read" }),
      line({ ts_ms: start, device_id: alpha, sentence: "First", tool: "files.read" }),
      line({ ts_ms: start - 61_000, device_id: alpha, sentence: "Older", tool: "files.read" })
    ];
    render(<ActivityTimeline lines={lines} devices={devices} />);
    expect(screen.getByText("3 calls · 1 m")).toBeInTheDocument();
  });
});

describe("ActivityTimeline unknown machines", () => {
  it("does not let an id the fleet lacks read like a machine", () => {
    // The live window has one line whose device id is a hostname from an old enrollment. The line is
    // real, so the chip exists — but it says "unknown machine", not a name that looks like a fleet.
    const ghost = line({
      ts_ms: Date.UTC(2026, 10, 5, 9, 5, 0),
      device_id: "desktop-9vh92bj",
      sentence: "Listed the plugins",
      tool: "plugins.list"
    });
    render(<ActivityTimeline lines={[ghost]} devices={devices} />);
    const group = screen.getByRole("group", { name: "Filter by machine" });
    expect(within(group).getByText("unknown machine · desktop-9vh9")).toBeInTheDocument();
    expect(within(group).queryByText("desktop-9vh92bj")).not.toBeInTheDocument();
  });
});

describe("ActivityTimeline", () => {
  it("opens a row's evidence when the row is clicked", () => {
    const run = line({
      tool: "runs.exec",
      sentence: "Ran `cargo test`",
      evidence: { command: "cargo", args: ["test"], output: "PASS 441 tests" }
    });
    render(<ActivityTimeline lines={[run]} devices={[]} />);

    expect(document.querySelector(".evidence")).toBeNull();
    openRow();

    // The command as the caller wrote it, one line, and the tail the device answered with.
    expect(screen.getByText("Command")).toBeInTheDocument();
    expect(screen.getByText("cargo test")).toBeInTheDocument();
    expect(screen.getByText("Output")).toBeInTheDocument();
    expect(screen.getByText(/PASS 441 tests/)).toBeInTheDocument();
    expect(screen.getByRole("button", { expanded: true })).toBeInTheDocument();
  });

  it("says an audit row has no evidence instead of drawing an empty panel", () => {
    const audited = line({
      tool: "files.read",
      sentence: "Read a file",
      // A row the page fetched from /audit: the file never carried arguments, so there is nothing here.
      evidence: undefined
    });
    render(<ActivityTimeline lines={[audited]} devices={[]} />);

    openRow();

    const panel = evidencePanel();
    expect(within(panel).getByText(/comes from the audit file/)).toBeInTheDocument();
    expect(within(panel).queryByText("Command")).not.toBeInTheDocument();
    expect(within(panel).queryByText("Output")).not.toBeInTheDocument();
  });

  it("shows a failed row's status in the evidence panel", () => {
    const failed = line({
      tool: "files.read",
      sentence: "Read notes.txt",
      status: 503,
      evidence: { path: "notes.txt" }
    });
    render(<ActivityTimeline lines={[failed]} devices={[]} />);

    openRow();

    const panel = evidencePanel();
    const status = within(panel).getByText("503");
    expect(status).toHaveClass("is-bad");
    expect(within(panel).getByText("Path")).toBeInTheDocument();
    const path = within(panel).getByText("notes.txt");
    // A long value is wrapped and keeps its whole text in the tooltip rather than being clipped.
    expect(path).toHaveAttribute("title", "notes.txt");
  });

  it("says a 200 that failed is failed, and keeps the code in the evidence", () => {
    // A device's JSON-RPC error crosses the console as HTTP 200. The row used to stand on "200",
    // which reads as success; the failure's own word is what the row says, and the code is still the
    // evidence panel's fact for anyone who needs it.
    const refused = line({
      tool: "memory.search",
      sentence: "Searched its memory for `deploy`",
      status: 200,
      ok: false,
      detail: "jsonrpc error -32002",
      caller: "client"
    });
    render(<ActivityTimeline lines={[refused]} devices={[]} />);

    const row = document.querySelector<HTMLElement>(".timeline-row");
    expect(row).not.toBeNull();
    expect(within(row as HTMLElement).getByText("failed")).toBeInTheDocument();
    expect(within(row as HTMLElement).queryByText("200")).toBeNull();

    openRow();

    expect(within(evidencePanel()).getByText("200")).toBeInTheDocument();
    expect(within(evidencePanel()).getByText("jsonrpc error -32002")).toBeInTheDocument();
  });

  it("keeps a successful row's presentation without an outcome word", () => {
    const succeeded = line({
      tool: "files.read",
      sentence: "Read notes.txt",
      status: 200,
      ok: true,
      caller: "client"
    });
    render(<ActivityTimeline lines={[succeeded]} devices={[]} />);

    expect(document.querySelector(".timeline-outcome")).toBeNull();
    expect(screen.getByText("Read notes.txt")).toBeInTheDocument();
  });

  it("shows a run's exit facts in the evidence panel", () => {
    // A run that failed silently is a failure the output alone cannot show; the exit facts ride in
    // the evidence object the proxy built.
    const run = line({
      tool: "runs.exec",
      sentence: "Ran `false`",
      evidence: { command: "false", exit_code: 1, timed_out: false }
    });
    render(<ActivityTimeline lines={[run]} devices={[]} />);

    openRow();

    const panel = evidencePanel();
    expect(within(panel).getByText("Exit")).toBeInTheDocument();
    expect(within(panel).getByText("1")).toBeInTheDocument();
    expect(within(panel).getByText("Timed out")).toBeInTheDocument();
    expect(within(panel).getByText("no")).toBeInTheDocument();
  });

  it("names the token behind a call in the evidence panel, and offers Manage", () => {
    const onManageToken = vi.fn();
    const attributed = line({
      tool: "files.read",
      sentence: "Read notes.txt",
      caller: "client",
      token_id: tokenA,
      evidence: { path: "notes.txt" }
    });
    render(
      <ActivityTimeline
        lines={[attributed]}
        devices={[]}
        tokens={tokens}
        onManageToken={onManageToken}
      />
    );

    openRow();

    const panel = evidencePanel();
    expect(within(panel).getByText("Token").nextElementSibling).toHaveTextContent("laptop-dsh");
    fireEvent.click(within(panel).getByRole("button", { name: "Manage" }));
    expect(onManageToken).toHaveBeenCalledWith(tokenA);
  });

  it("says signed grant for a client's call with no token record", () => {
    const signed = line({
      tool: "files.read",
      sentence: "Read notes.txt",
      caller: "client",
      evidence: { path: "notes.txt" }
    });
    render(<ActivityTimeline lines={[signed]} devices={[]} tokens={[]} />);

    openRow();

    expect(evidencePanel().textContent).toContain("signed grant");
    expect(evidencePanel().textContent).not.toContain("operator session");
  });

  it("says operator session only for the console's own call", () => {
    const consoleRead = line({
      tool: "terminal.list",
      sentence: "Listed the terminal sessions",
      caller: "console",
      evidence: {}
    });
    render(<ActivityTimeline lines={[consoleRead]} devices={[]} tokens={[]} />);

    openRow();

    expect(evidencePanel().textContent).toContain("operator session");
  });

  it("fails a device refusal that came back as HTTP 200 and shows its detail", () => {    // A device's JSON-RPC error is HTTP 200, so the status alone says nothing. The live event's
    // `ok` and the device's own message are what make the row read as a refusal.
    const refused = line({
      tool: "files.read",
      sentence: "Read notes.txt",
      status: 200,
      ok: false,
      detail: "tool not permitted",
      evidence: { path: "notes.txt" }
    });
    render(<ActivityTimeline lines={[refused]} devices={[]} />);

    const dot = document.querySelector(".timeline-dot");
    expect(dot).toHaveAttribute("data-tone", "bad");

    openRow();

    const panel = evidencePanel();
    const detail = within(panel).getByText("tool not permitted");
    expect(detail).toHaveClass("is-bad");
  });

  it("opens a live row with a real button", () => {
    const run = line({
      tool: "files.read",
      sentence: "Read notes.txt",
      evidence: { path: "notes.txt" }
    });
    render(<ActivityTimeline lines={[run]} devices={[]} />);

    const button = screen.getByRole("button", { expanded: false });
    expect(button.tagName).toBe("BUTTON");
    fireEvent.click(button);
    expect(button).toHaveAttribute("aria-expanded", "true");
  });

  it("does not claim audit provenance for a live tool with no evidence", () => {
    // `host.facts` has no evidence template, so its live event carries none — but it is still a
    // live call, not a row read back from the audit file.
    const live = line({
      live: true,
      tool: "host.facts",
      sentence: "Read the machine's facts"
    });
    render(<ActivityTimeline lines={[live]} devices={[]} />);

    openRow();

    const panel = evidencePanel();
    expect(within(panel).getByText("Status")).toBeInTheDocument();
    expect(within(panel).getByText("host.facts")).toBeInTheDocument();
    expect(within(panel).queryByText(/audit file/)).not.toBeInTheDocument();
    expect(within(panel).getByText(/reports its result only/i)).toBeInTheDocument();
  });

  it("shows a captured frame, when it was taken and that memory forgets it", () => {
    // A screenshot is the one piece of evidence that cannot travel in the event: the row carries a
    // same-origin path, and the panel is where the operator finally sees the picture.
    const capture = line({
      tool: "screen.capture",
      live: true,
      sentence: "Took a screenshot of the screen",
      evidence: {
        shot: "/activity/shot/device-1/image-1",
        captured_at_ms: Date.UTC(2026, 10, 5, 9, 0, 5)
      }
    });
    render(<ActivityTimeline lines={[capture]} devices={[]} />);

    openRow();

    const panel = evidencePanel();
    const image = within(panel).getByRole("img");
    expect(image).toHaveAttribute("src", "/activity/shot/device-1/image-1");
    expect(image).toHaveAttribute("loading", "lazy");
    expect(within(panel).getByText(/^Captured at \d{2}:\d{2}:\d{2}$/)).toBeInTheDocument();
    expect(
      within(panel).getByText(/Kept in the console's memory; a restart forgets it\./)
    ).toBeInTheDocument();
  });

  it("draws no frame when a line carries no shot", () => {
    const read = line({
      tool: "files.read",
      sentence: "Read notes.txt",
      evidence: { path: "notes.txt" }
    });
    render(<ActivityTimeline lines={[read]} devices={[]} />);

    openRow();

    expect(within(evidencePanel()).queryByRole("img")).toBeNull();
  });

  it("says a frame could not be loaded rather than leaving a broken image", () => {
    const capture = line({
      tool: "screen.capture",
      live: true,
      sentence: "Took a screenshot of the screen",
      evidence: { shot: "/activity/shot/device-1/gone", captured_at_ms: Date.UTC(2026, 10, 5, 9, 0, 5) }
    });
    render(<ActivityTimeline lines={[capture]} devices={[]} />);

    openRow();

    const panel = evidencePanel();
    fireEvent.error(within(panel).getByRole("img"));

    expect(within(panel).queryByRole("img")).toBeNull();
    expect(within(panel).getByText(/could not be loaded/i)).toBeInTheDocument();
  });

  it("splits the clock for the phone rules, and keeps the age in its own element", () => {
    // The row's structure, not a `matchMedia` reading: at <=480 px the stylesheet hides the seconds
    // span and narrows the time cell, and hides only the age — the machine stays beside it.
    const ts = Date.UTC(2026, 10, 5, 9, 0, 5);
    render(
      <ActivityTimeline
        lines={[line({ ts_ms: ts, device_id: alpha, sentence: "Read notes.txt" })]}
        devices={devices}
        nowMs={ts + 60_000}
      />
    );

    const time = document.querySelector(".timeline-time");
    expect(time).not.toBeNull();
    const clock = time?.querySelector(".timeline-clock");
    const seconds = time?.querySelector(".timeline-clock-seconds");
    expect(clock).toHaveTextContent(/^\d{2}:\d{2}$/);
    expect(seconds).toHaveTextContent(/^:\d{2}$/);
    // The whole instant is still in the element and in `dateTime`; only its drawing is split.
    expect(time).toHaveTextContent(/^\d{2}:\d{2}:\d{2}$/);
    expect(time).toHaveAttribute("dateTime", new Date(ts).toISOString());

    const facts = document.querySelector(".timeline-facts");
    expect(facts?.querySelector(".timeline-age")).toHaveTextContent(/ago$/);
    expect(facts?.querySelector(".timeline-machine")).toHaveTextContent("Alpha");
  });

  it("marks the day label for the scroll container so it can stick", () => {
    render(<ActivityTimeline lines={[fromAlpha]} devices={[]} nowMs={fromAlpha.ts_ms + 1_000} />);

    const label = screen.getByRole("heading", { name: "Today" });
    expect(label).toHaveClass("timeline-day-label");
    expect(label).toHaveClass("is-sticky");
  });

  it("freezes the age column at nowMs so a paused page stops ageing", () => {
    vi.useFakeTimers();
    try {
      const ts = Date.UTC(2026, 10, 5, 9, 0, 0);
      vi.setSystemTime(ts + 5_000);
      const startInterval = vi.spyOn(window, "setInterval");
      render(
        <ActivityTimeline
          lines={[line({ ts_ms: ts, sentence: "Read notes.txt" })]}
          devices={[]}
          nowMs={ts + 5_000}
        />
      );
      expect(screen.getByText("5s ago")).toBeInTheDocument();
      expect(
        startInterval.mock.calls.some(([, delay]) => delay === 1000),
        "a paused page must not keep a 1 s ticker"
      ).toBe(false);

      act(() => {
        vi.advanceTimersByTime(60_000);
      });
      expect(screen.getByText("5s ago")).toBeInTheDocument();
      startInterval.mockRestore();
    } finally {
      vi.useRealTimers();
    }
  });

  it("narrows the rows to a machine chip and restores them with All machines", () => {
    render(<ActivityTimeline lines={[fromAlpha, fromBeta]} devices={devices} />);

    expect(screen.getByText("Read notes.txt")).toBeInTheDocument();
    expect(screen.getByText("Ran a command")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Beta" }));

    expect(screen.queryByText("Read notes.txt")).not.toBeInTheDocument();
    expect(screen.getByText("Ran a command")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Beta" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "All machines" })).toHaveAttribute(
      "aria-pressed",
      "false"
    );

    fireEvent.click(screen.getByRole("button", { name: "All machines" }));

    expect(screen.getByText("Read notes.txt")).toBeInTheDocument();
    expect(screen.getByText("Ran a command")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "All machines" })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
  });

  it("keeps a device filter visible as a chip when this window has no line for it", () => {
    // The table can narrow to a machine whose lines are not in the window the page fetched. The
    // timeline must still show that filter, rather than reading as All machines and lying about it.
    render(<ActivityTimeline lines={[fromAlpha]} devices={devices} device={beta} />);

    const chips = screen.getByRole("group", { name: "Filter by machine" });
    expect(within(chips).getByRole("button", { name: "Beta" })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
    expect(within(chips).getByRole("button", { name: "All machines" })).toHaveAttribute(
      "aria-pressed",
      "false"
    );
    expect(screen.getByText(/No activity on Beta/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear machine filter" })).toBeInTheDocument();
  });

  it("searches sentence, tool, machine and detail, and names a query that hides everything", () => {
    render(<ActivityTimeline lines={[fromAlpha, fromBeta]} devices={devices} />);
    const search = screen.getByRole("searchbox", { name: "Search activity" });

    // The sentence, case-insensitively.
    fireEvent.change(search, { target: { value: "COMMAND" } });
    expect(screen.getByText("Ran a command")).toBeInTheDocument();
    expect(screen.queryByText("Read notes.txt")).not.toBeInTheDocument();

    // The tool.
    fireEvent.change(search, { target: { value: "files.read" } });
    expect(screen.getByText("Read notes.txt")).toBeInTheDocument();
    expect(screen.queryByText("Ran a command")).not.toBeInTheDocument();

    // The fleet's name for the machine.
    fireEvent.change(search, { target: { value: "alpha" } });
    expect(screen.getByText("Read notes.txt")).toBeInTheDocument();
    expect(screen.queryByText("Ran a command")).not.toBeInTheDocument();

    // The detail.
    fireEvent.change(search, { target: { value: "exit 0" } });
    expect(screen.getByText("Ran a command")).toBeInTheDocument();
    expect(screen.queryByText("Read notes.txt")).not.toBeInTheDocument();

    // Nothing matches: the page says what was searched and keeps the field, so it can be cleared.
    fireEvent.change(search, { target: { value: "no-such-line" } });
    expect(screen.queryByText("Read notes.txt")).not.toBeInTheDocument();
    expect(screen.queryByText("Ran a command")).not.toBeInTheDocument();
    expect(screen.getByText(/No activity matches/)).toBeInTheDocument();
    expect(screen.getByText(/no-such-line/)).toBeInTheDocument();
    expect(screen.getByRole("searchbox", { name: "Search activity" })).toHaveValue("no-such-line");
    expect(screen.getByRole("group", { name: "Filter by machine" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));

    expect(screen.getByText("Read notes.txt")).toBeInTheDocument();
    expect(screen.getByText("Ran a command")).toBeInTheDocument();
    expect(screen.getByRole("searchbox", { name: "Search activity" })).toHaveValue("");
  });

  it("says what this page is and how its first line gets there when it has none", () => {
    render(<ActivityTimeline lines={[]} devices={[]} />);
    expect(screen.getByText(/No activity yet\./)).toBeInTheDocument();
    // The record names its own boundary: a call straight to the machine is not in it.
    expect(
      screen.getByText(/straight to a machine's own endpoint is not/)
    ).toBeInTheDocument();
    expect(screen.getByText(/Connect a client/)).toBeInTheDocument();
    expect(screen.queryByText("Nothing has happened yet.")).not.toBeInTheDocument();
  });

  it("draws a full 1000-row day, down to the last row", () => {
    // P2-12 measured this: the page asks for the server's cap and the timeline draws all of it.
    // The fixture is one day, newest first, with a unique timestamp so every row has its own key.
    const newest = Date.UTC(2026, 10, 5, 9, 0, 0);
    const lines = Array.from({ length: 1000 }, (_, index) =>
      line({
        ts_ms: newest - index * 1_000,
        sentence: `Call ${index + 1}`,
        tool: "files.read"
      })
    );
    render(<ActivityTimeline lines={lines} devices={[]} nowMs={newest + 1_000} />);

    expect(document.querySelectorAll(".timeline-row")).toHaveLength(1000);
    expect(screen.getByText("Call 1000")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Today" })).toBeInTheDocument();
  });

  it("marks the console's own reads, and not a client's call", () => {
    // The console polls `terminal.list` and parks `terminal.read` on its own behalf. Those rows stay
    // in the record — they are calls that happened — but a reader must be able to tell them from the
    // AI's work without opening the evidence.
    const consoleLine = line({
      ts_ms: Date.UTC(2026, 10, 5, 9, 2, 0),
      device_id: alpha,
      sentence: "Listed the terminal sessions",
      tool: "terminal.list",
      caller: "console"
    });
    const clientLine = line({
      ts_ms: Date.UTC(2026, 10, 5, 9, 1, 0),
      device_id: alpha,
      sentence: "Read notes.txt",
      tool: "files.read",
      caller: "client"
    });
    render(<ActivityTimeline lines={[consoleLine, clientLine]} devices={devices} />);

    const rows = Array.from(document.querySelectorAll<HTMLElement>(".timeline-row"));
    const consoleRow = rows.find((row) => row.textContent?.includes("Listed the terminal sessions"));
    const clientRow = rows.find((row) => row.textContent?.includes("Read notes.txt"));
    expect(consoleRow?.querySelector(".timeline-caller")).toHaveTextContent("console");
    expect(clientRow?.querySelector(".timeline-caller")).toBeNull();
  });

  it("says the window was capped only when it was", () => {
    const { rerender } = render(<ActivityTimeline lines={[fromAlpha]} devices={[]} />);
    expect(screen.queryByText(/showing the newest 1000/i)).toBeNull();

    rerender(<ActivityTimeline lines={[fromAlpha]} devices={[]} capped />);
    // The list spans days, so the sentence names the window and never "of today": the rows behind
    // the cut can be yesterday's, and that word would then be false.
    expect(
      screen.getByText("Showing the newest 1000 lines; older ones are not on this page.")
    ).toBeInTheDocument();
    expect(screen.queryByText(/of today/)).toBeNull();
  });

  it("names the machine whose window was cut", () => {
    render(<ActivityTimeline lines={[fromAlpha]} devices={devices} capped cappedMachine="Alpha" />);
    expect(
      screen.getByText("Showing the newest 1000 lines from Alpha; older ones are not on this page.")
    ).toBeInTheDocument();
  });

  it("says the window's limit when the request came back full", () => {
    const { rerender } = render(<ActivityTimeline lines={[fromAlpha]} devices={[]} />);
    expect(screen.queryByText(/the window's limit/)).toBeNull();

    // The window filled the server's limit; nothing was dropped here, so the sentence must not
    // claim older lines exist. The table view reads the same fact.
    rerender(<ActivityTimeline lines={[fromAlpha]} devices={[]} atLimit />);
    expect(
      screen.getByText("Showing the newest 1000 lines — the window's limit.")
    ).toBeInTheDocument();
  });

  it("reads the new window in one line instead of drawing the old window's rows", () => {
    render(
      <ActivityTimeline
        lines={[fromAlpha]}
        devices={devices}
        device={beta}
        loading
        loadingMachine="Beta"
      />
    );

    // The chip the operator just chose is pressed, and the list says what is happening; the rows in
    // hand belong to another machine's window, so drawing them under this chip would be the wrong
    // answer even before the fetch lands.
    expect(screen.getByRole("button", { name: "Beta" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("Reading Beta's window…")).toBeInTheDocument();
    expect(screen.queryByText("Read notes.txt")).toBeNull();
    expect(screen.queryByText(/No activity on Beta/)).toBeNull();
  });

  it("names the fleet when the window being read is the fleet's", () => {
    render(<ActivityTimeline lines={[fromAlpha]} devices={devices} loading />);
    expect(screen.getByText("Reading the fleet's window…")).toBeInTheDocument();
  });
});
