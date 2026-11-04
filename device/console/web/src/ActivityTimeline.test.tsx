import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AuditLine, Device } from "./api";
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

  it("says the list was capped only when it was", () => {
    const { rerender } = render(<ActivityTimeline lines={[fromAlpha]} devices={[]} />);
    expect(screen.queryByText(/showing the newest 1000/i)).toBeNull();

    rerender(<ActivityTimeline lines={[fromAlpha]} devices={[]} capped />);
    expect(screen.getByText("Showing the newest 1000 of today.")).toBeInTheDocument();
  });
});
