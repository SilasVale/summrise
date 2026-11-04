import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AuditLine } from "./api";
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
});
