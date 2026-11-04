import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
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
    expect(within(panel).getByText("notes.txt")).toBeInTheDocument();
  });
});
