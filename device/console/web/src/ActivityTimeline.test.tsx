import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditLine, AuditSource, Device, TokenMeta } from "./api";
import { ActivityTimeline } from "./ActivityTimeline";
import { exactMoment } from "./format";
import { restoreGlobal } from "./test/dom";

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

// A device id is 64 hex characters. The audit's two retired ids are the shape `machineOptions` may
// call an old id (audit-39 F2); a hostname is not.
const retiredHex = "fe209fc2c119e368ee8e36eeb211984e07ab99a4c867d984f71b23562f797d82";
const goneHex = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";

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

  it("puts a long stretch's summary above its first row, and keeps no footer for it", () => {
    // The measured stretch was 470 rows and its summary arrived after them, where the only way to
    // use it was to walk the rows it described. Above a threshold the summary is the header of the
    // stretch: the next row the reader meets is the first row it counts.
    const start = Date.UTC(2026, 10, 5, 9, 0, 0);
    const long = Array.from({ length: 12 }, (_, index) =>
      line({
        ts_ms: start - index * 10_000,
        device_id: alpha,
        sentence: `Long ${index + 1}`,
        tool: "files.read"
      })
    );
    const older = line({
      ts_ms: start - 11 * 10_000 - 61_000,
      device_id: alpha,
      sentence: "Older",
      tool: "files.read"
    });
    render(<ActivityTimeline lines={[...long, older]} devices={devices} />);

    const header = document.querySelector<HTMLElement>(".timeline-stretch.is-header");
    expect(header).not.toBeNull();
    expect(header).toHaveTextContent("12 calls · 1 m 50 s");
    // It belongs to the list, not to the copy: the item after it is the stretch's first row.
    expect(header?.nextElementSibling).toHaveTextContent("Long 1");
    // The summary moved rather than copied: a long stretch is not summarized a second time below.
    expect(document.querySelectorAll(".timeline-stretch")).toHaveLength(1);
  });

  it("keeps a short stretch's summary inline after its rows", () => {
    // Below the threshold the old form stays: the label sits where the stretch ends, so a day of
    // short bursts still reads as boundaries without a header over every two rows.
    const start = Date.UTC(2026, 10, 5, 9, 0, 0);
    const lines = [
      line({ ts_ms: start, device_id: alpha, sentence: "Newest 1", tool: "files.read" }),
      line({ ts_ms: start - 10_000, device_id: alpha, sentence: "Newest 2", tool: "files.read" }),
      line({ ts_ms: start - 71_000, device_id: alpha, sentence: "Older 1", tool: "files.read" }),
      line({ ts_ms: start - 81_000, device_id: alpha, sentence: "Older 2", tool: "files.read" })
    ];
    render(<ActivityTimeline lines={lines} devices={devices} />);

    expect(document.querySelector(".timeline-stretch.is-header")).toBeNull();
    const footer = screen.getByText("2 calls · 10 s").closest("li");
    expect(footer).not.toBeNull();
    expect(footer).not.toHaveClass("is-header");
    expect(footer?.previousElementSibling).toHaveTextContent("Newest 2");
  });

  it("draws no summary for a stretch that held one call", () => {
    // "1 call" is the row above it said again. Twenty-eight of the live page's sixty-four labels
    // were exactly that noise, so a one-call stretch is drawn without one.
    const start = Date.UTC(2026, 10, 5, 9, 0, 0);
    const lines = [
      line({ ts_ms: start, device_id: alpha, sentence: "Newest 1", tool: "files.read" }),
      line({ ts_ms: start - 10_000, device_id: alpha, sentence: "Newest 2", tool: "files.read" }),
      line({ ts_ms: start - 71_000, device_id: alpha, sentence: "Single", tool: "files.read" }),
      line({ ts_ms: start - 132_000, device_id: alpha, sentence: "Older 1", tool: "files.read" }),
      line({ ts_ms: start - 137_000, device_id: alpha, sentence: "Older 2", tool: "files.read" })
    ];
    render(<ActivityTimeline lines={lines} devices={devices} />);

    expect(screen.queryByText("1 call")).toBeNull();
    // The stretch above it keeps its summary: only the one-call label is dropped.
    expect(screen.getByText("2 calls · 10 s")).toBeInTheDocument();
    expect(screen.getByText("Single")).toBeInTheDocument();
  });
});

describe("ActivityTimeline ids the fleet no longer lists", () => {
  it("dates a 64-hex id the fleet no longer lists as an old id", () => {
    // An id is 64 hex characters. A value of that shape that the fleet does not list is a retired
    // id: the line is real, so it keeps a chip — dated by its newest line, not called a machine.
    const ghost = line({
      ts_ms: Date.UTC(2026, 10, 5, 9, 5, 0),
      device_id: retiredHex,
      sentence: "Listed the plugins",
      tool: "plugins.list"
    });
    render(
      <ActivityTimeline
        lines={[ghost]}
        devices={devices}
        nowMs={Date.UTC(2026, 10, 5, 9, 10, 0)}
      />
    );
    const group = screen.getByRole("group", { name: "Filter by machine" });
    expect(within(group).queryByText(/unknown machine/)).toBeNull();
    const chip = within(group).getByRole("button", {
      name: "old id · fe209fc2c119… · 5m ago"
    });
    expect(chip).toHaveAttribute(
      "title",
      `not in the fleet: ${retiredHex}; newest line 5m ago`
    );
    expect(screen.getByText("Listed the plugins")).toBeInTheDocument();
  });

  it("matches a refusal's device name to the machine that carries it", () => {
    // audit-39 F2: a 403 `device_not_permitted` records the requested machine's *name* in
    // `device_id`. The fleet knows DESKTOP-84T3LIN, so the row is that machine's; a second chip
    // reading `old id · DESKTOP-84T3… · 37m ago` with `not in the fleet: DESKTOP-84T3LIN` called a
    // live machine retired on the page whose job is the record.
    const now = Date.UTC(2026, 10, 5, 9, 10, 0);
    const named: Device[] = [{ ...devices[0], name: "DESKTOP-84T3LIN" }, devices[1]];
    const deny = line({
      ts_ms: now - 37 * 60_000,
      device_id: "DESKTOP-84T3LIN",
      sentence: "Refused a request",
      detail: "device_not_permitted",
      status: 403
    });
    render(<ActivityTimeline lines={[deny, fromBeta]} devices={named} nowMs={now} />);

    const group = screen.getByRole("group", { name: "Filter by machine" });
    // The machine has one chip, under its fleet id; nothing calls it retired or absent.
    expect(within(group).queryByText(/old id/)).toBeNull();
    for (const chip of within(group).getAllByRole("button")) {
      expect(chip.getAttribute("title") ?? "").not.toContain("not in the fleet");
    }
    const chip = within(group).getByRole("button", { name: "DESKTOP-84T3LIN" });
    expect(chip).toHaveAttribute("title", alpha);
    // The row is that machine's, named by the fleet rather than by a truncated denial.
    const row = screen.getByText("Refused a request").closest("li");
    expect(row?.querySelector(".timeline-machine")).toHaveTextContent("DESKTOP-84T3LIN");
    // Pressing the machine's chip scopes to it: its refusal stays, the other machine's row goes.
    fireEvent.click(chip);
    expect(chip).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("Refused a request")).toBeInTheDocument();
    expect(screen.queryByText("Ran a command")).not.toBeInTheDocument();
  });

  it("leaves a name the fleet does not carry to the record, not to an old id", () => {
    // The other half of the ruling: a name is matched to a machine the fleet has, or left in the
    // record that wrote it. It is not an id whose retired history a chip could scope.
    const unknownName = "DESKTOP-ZZZZZZZ";
    const deny = line({
      ts_ms: Date.UTC(2026, 10, 5, 9, 5, 0),
      device_id: unknownName,
      sentence: "Refused a request",
      detail: "device_not_permitted",
      status: 403
    });
    render(
      <ActivityTimeline
        lines={[deny]}
        devices={devices}
        nowMs={Date.UTC(2026, 10, 5, 9, 10, 0)}
      />
    );
    const group = screen.getByRole("group", { name: "Filter by machine" });
    expect(within(group).queryByText(/old id/)).toBeNull();
    expect(within(group).queryByRole("button", { name: new RegExp(unknownName) })).toBeNull();
    for (const chip of within(group).getAllByRole("button")) {
      expect(chip.getAttribute("title") ?? "").not.toContain(unknownName);
    }
    // The row is still the record, and it carries the name it was written with.
    const row = screen.getByText("Refused a request").closest("li");
    expect(row?.querySelector(".timeline-machine")).toHaveTextContent("DESKTOP-ZZZZ…");
  });

  it("gives the selected gone id no chip and the ux95 window's old id a dated one", () => {
    // ux95 measured this shape with the selected id deadbeefdead: the chips drew
    // `unknown machine · deadbeefdead` (the fallback's id) and `unknown machine · fe209…`
    // (an old audit line). The fallback sentence is the only place the selected id is named; the
    // line-derived id keeps a scope but says it is old and how old its newest line is.
    const now = Date.UTC(2026, 10, 5, 9, 0, 0);
    const gone = line({
      ts_ms: now - 300_000,
      device_id: goneHex,
      sentence: "Read a removed machine's notes",
      tool: "files.list"
    });
    const old = line({
      ts_ms: now - 120_000,
      device_id: retiredHex,
      sentence: "Refused a request",
      tool: "files.read",
      status: 403
    });
    render(<ActivityTimeline lines={[gone, old]} devices={devices} goneId={goneHex} nowMs={now} />);

    const group = screen.getByRole("group", { name: "Filter by machine" });
    expect(within(group).queryByText(/unknown machine/)).toBeNull();
    expect(within(group).queryByRole("button", { name: /deadbeef/ })).toBeNull();
    const oldChip = within(group).getByRole("button", {
      name: "old id · fe209fc2c119… · 2m ago"
    });
    expect(oldChip).toHaveAttribute(
      "title",
      `not in the fleet: ${retiredHex}; newest line 2m ago`
    );
    // Known machines are unchanged: the fleet's name, not a prefix, and the id in the title.
    expect(within(group).getByRole("button", { name: "Alpha" })).toHaveAttribute("title", alpha);
    // The rows stay, and the old id is still a scope: pressing it narrows to that id's history.
    expect(screen.getByText("Read a removed machine's notes")).toBeInTheDocument();
    fireEvent.click(oldChip);
    expect(screen.queryByText("Read a removed machine's notes")).not.toBeInTheDocument();
    expect(screen.getByText("Refused a request")).toBeInTheDocument();
    expect(oldChip).toHaveAttribute("aria-pressed", "true");
  });

  it("shows a table's old-id filter as an old id, not a missing machine", () => {
    // The table's device select offers an id the fleet does not list, so the shared filter can be
    // one while the timeline's window has no line for it. The chip that keeps that filter visible
    // must not invent an age the window cannot prove.
    render(<ActivityTimeline lines={[fromAlpha]} devices={devices} device={retiredHex} />);
    const group = screen.getByRole("group", { name: "Filter by machine" });
    expect(within(group).queryByText(/unknown machine/)).toBeNull();
    const chip = within(group).getByRole("button", { name: "old id · fe209fc2c119…" });
    expect(chip).toHaveAttribute("aria-pressed", "true");
    expect(chip).toHaveAttribute(
      "title",
      `not in the fleet: ${retiredHex}; no line in this window`
    );
    expect(screen.getByText("No activity on fe209fc2c119….")).toBeInTheDocument();
  });
});

describe("ActivityTimeline channel boundaries", () => {
  it("does not draw the console's device.left and device.joined lines as rows", () => {
    // The audit holds the two moments of a channel so a fleet row can measure an absence. They are
    // console facts, not calls: the timeline is a list of work, so the boundary stays out of it.
    const left = line({
      ts_ms: Date.UTC(2026, 10, 5, 8, 0, 0),
      kind: "device",
      device_id: alpha,
      tool: "device.left",
      caller: "console",
      sentence: "The channel closed"
    });
    const joined = line({
      ts_ms: Date.UTC(2026, 10, 5, 9, 0, 0),
      kind: "device",
      device_id: alpha,
      tool: "device.joined",
      caller: "console",
      sentence: "The channel opened"
    });
    render(<ActivityTimeline lines={[joined, left, fromAlpha]} devices={devices} />);

    expect(screen.queryByText("The channel closed")).not.toBeInTheDocument();
    expect(screen.queryByText("The channel opened")).not.toBeInTheDocument();
    expect(screen.queryByText("device.left")).not.toBeInTheDocument();
    expect(screen.queryByText("device.joined")).not.toBeInTheDocument();
    expect(screen.getByText("Read notes.txt")).toBeInTheDocument();
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

  it("says a tool-level 424 is failed and keeps the reason", () => {
    // A tool that answers isError is a failed call, and the console records it as 424. The row must
    // say "failed" and keep the device's reason in the evidence, because that is the same fact the
    // audit line and the run form name.
    const refused = line({
      tool: "runs.exec",
      sentence: "Could not run the command",
      status: 424,
      ok: false,
      detail: "runs.exec: program not found",
      caller: "client"
    });
    render(<ActivityTimeline lines={[refused]} devices={[]} />);

    const row = document.querySelector<HTMLElement>(".timeline-row");
    expect(row).not.toBeNull();
    expect(within(row as HTMLElement).getByText("failed")).toBeInTheDocument();
    expect(within(row as HTMLElement).queryByText("424")).toBeNull();

    openRow();

    const panel = evidencePanel();
    expect(within(panel).getByText("424")).toBeInTheDocument();
    expect(within(panel).getByText("runs.exec: program not found")).toBeInTheDocument();
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

  it("keeps the exact moment in the clock's title, so the phone's seconds are a convenience", () => {
    // audit-43 F8: the Timeline's `<time>` had no title, so the instant was nowhere; it also drew
    // only `08:14` at 390. The seconds stay drawn now (the gate measures that), and the title is
    // the record either way.
    const ts = Date.UTC(2026, 10, 5, 9, 0, 5);
    render(
      <ActivityTimeline
        lines={[line({ ts_ms: ts, device_id: alpha, sentence: "Read notes.txt" })]}
        devices={devices}
        nowMs={ts + 8_000}
      />
    );

    const time = document.querySelector(".timeline-time") as HTMLTimeElement;
    expect(time).not.toBeNull();
    expect(time).toHaveAttribute("title", exactMoment(ts));
    expect(time).toHaveTextContent(/^\d{2}:\d{2}:\d{2}$/);
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

  it("narrows to the AI's calls with one chip and restores the whole record", () => {
    // The timeline's machine chips cannot separate the AI from the console or from a row with no
    // proof at all. One chip does, using the rule the day's count uses, and says how many rows it
    // will narrow to before it is pressed.
    const client = line({
      ts_ms: Date.UTC(2026, 10, 5, 9, 0, 0),
      device_id: alpha,
      sentence: "Read notes.txt",
      tool: "files.read",
      caller: "client"
    });
    const provenByToken = line({
      ts_ms: Date.UTC(2026, 10, 5, 9, 1, 0),
      device_id: alpha,
      sentence: "Ran a command",
      tool: "runs.exec",
      token_id: tokenA
    });
    const consoleRead = line({
      ts_ms: Date.UTC(2026, 10, 5, 9, 2, 0),
      device_id: alpha,
      sentence: "Listed the terminal sessions",
      tool: "terminal.list",
      caller: "console"
    });
    const unattributed = line({
      ts_ms: Date.UTC(2026, 10, 5, 9, 3, 0),
      device_id: alpha,
      sentence: "Registered a machine",
      kind: "register"
    });
    render(
      <ActivityTimeline
        lines={[unattributed, consoleRead, provenByToken, client]}
        devices={devices}
      />
    );

    const chip = screen.getByRole("button", { name: "Clients only" });
    expect(chip).toHaveAttribute("aria-pressed", "false");
    expect(chip).toHaveAccessibleDescription("2 client calls in this window · from 09:00, spanning 3 m");
    // Unpressed the whole record is drawn: two of these rows are not the AI's calls.
    expect(document.querySelectorAll(".timeline-row")).toHaveLength(4);

    fireEvent.click(chip);

    expect(chip).toHaveAttribute("aria-pressed", "true");
    // Pressed, the subset is the list, so the count's noun is `rows` — the same number and window,
    // named for what is now drawn rather than for the caller filter that selected it.
    expect(chip).toHaveAccessibleDescription("2 rows in this window · from 09:00, spanning 3 m");
    expect(document.querySelectorAll(".timeline-row")).toHaveLength(2);
    expect(screen.getByText("Read notes.txt")).toBeInTheDocument();
    expect(screen.getByText("Ran a command")).toBeInTheDocument();
    expect(screen.queryByText("Listed the terminal sessions")).toBeNull();
    expect(screen.queryByText("Registered a machine")).toBeNull();

    fireEvent.click(chip);

    expect(chip).toHaveAttribute("aria-pressed", "false");
    expect(document.querySelectorAll(".timeline-row")).toHaveLength(4);
    expect(screen.getByText("Listed the terminal sessions")).toBeInTheDocument();
    expect(screen.getByText("Registered a machine")).toBeInTheDocument();
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

  it("draws a full 1000-row day behind one door, down to the last row", () => {
    // P2-12 measured this: the page asks for the server's cap. The day is not thrown away, but it is
    // not a 70,000 px wall either: the newest 60 rows are drawn and the other 940 are a door that
    // opens the rest in place (backlog item 2).
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

    // The record is the whole day, but the page is not a wall: the newest 60 are drawn and the
    // other 940 are one door. Pressing it draws the day down to its last row (backlog item 2).
    expect(document.querySelectorAll(".timeline-row")).toHaveLength(60);
    expect(screen.queryByText("Call 1000")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /^Show the rest of today/ }));
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

  it("names what was drawn and what the merge withheld from the fetched window", () => {
    // The measured Activity 1440 window: 1000 fetched, 34 channel boundaries, 17 repeated keys.
    // The sentence names the 949 rows a reader gets, not the fetch's 1000.
    const counts = { fetched: 1000, drawn: 949, boundaries: 34, repeats: 17 };
    render(
      <ActivityTimeline lines={[fromAlpha]} devices={[]} atLimit windowCounts={counts} />
    );

    expect(
      screen.getByText(
        "Showing the newest 949 lines of 1000 fetched (34 channel events, 17 repeats not drawn)."
      )
    ).toBeInTheDocument();
    expect(screen.queryByText(/the window's limit/)).toBeNull();
  });

  it("says which file the window is read from, and nothing when that fact is absent", () => {
    // The cap sentence says what is drawn; this line says what is read. It carries the server's
    // facts only — a page that never reached `/audit/source` must draw no line at all.
    const source: AuditSource = {
      file: "audit.jsonl",
      bytes: 8_808_038,
      oldest_ts_ms: new Date(2026, 10, 2, 9, 12).getTime(),
      newest_ts_ms: new Date(2026, 10, 5, 17, 0).getTime(),
      max_bytes: 8_388_608,
      rotations: 1,
      rotated: {
        file: "audit.1.jsonl",
        oldest_ts_ms: new Date(2026, 10, 2, 3, 10).getTime()
      }
    };
    // The file's lifetime and the window's span are two facts, and the line states both apart: the
    // measured defect printed the file's 17-day lifetime beside the newest-1000 cap and let it read
    // as this page's range (audit-44 F10). The window's ends come from the drawn lines, not from
    // `/audit/source`'s own oldest/newest.
    const inWindow = { ...fromAlpha, ts_ms: new Date(2026, 10, 5, 9, 0, 0).getTime() };
    const { rerender } = render(
      <ActivityTimeline lines={[inWindow]} devices={[]} atLimit source={source} />
    );
    expect(
      screen.getByText(
        "Reading audit.jsonl — 8.4 MiB, lines since 11-02 09:12; rotated once, older lines start 11-02 03:10. " +
          "This window: 11-05 09:00."
      )
    ).toBeInTheDocument();

    rerender(<ActivityTimeline lines={[fromAlpha]} devices={[]} atLimit />);
    expect(screen.queryByText(/^Reading audit\.jsonl/)).toBeNull();
  });

  it("names the machine and the cut beside the withheld counts", () => {
    render(
      <ActivityTimeline
        lines={[fromAlpha]}
        devices={devices}
        capped
        cappedMachine="Alpha"
        windowCounts={{ fetched: 1000, drawn: 949, boundaries: 34, repeats: 17 }}
      />
    );

    expect(
      screen.getByText(
        "Showing the newest 949 lines of 1000 fetched from Alpha (34 channel events, 17 repeats not drawn); older ones are not on this page."
      )
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
    // **THE LOADING STATE IS THE ATOM'S SHAPE, NOT A SENTENCE (the eightieth list).** Its label lives on
    // the live region, so the read is announced and the words are the same ones the page used to print.
    expect(screen.getByLabelText("Reading Beta's window…")).toBeInTheDocument();
    expect(screen.getByLabelText("Reading Beta's window…")).toHaveAttribute("role", "status");
    expect(screen.getAllByRole("status").some((el) =>
      el.classList.contains("skeleton-list")).valueOf()).toBe(true);
    expect(screen.queryByText("Read notes.txt")).toBeNull();
    expect(screen.queryByText(/No activity on Beta/)).toBeNull();
  });

  it("names the fleet when the window being read is the fleet's", () => {
    render(<ActivityTimeline lines={[fromAlpha]} devices={devices} loading />);
    expect(screen.getByLabelText("Reading the fleet's window…")).toBeInTheDocument();
  });
});

describe("ActivityTimeline's labelled filter and record footer", () => {
  const clientAlpha = line({
    ts_ms: Date.UTC(2026, 10, 5, 9, 0, 0),
    device_id: alpha,
    sentence: "Read notes.txt",
    tool: "files.read",
    caller: "client"
  });
  const clientBeta = line({
    ts_ms: Date.UTC(2026, 10, 5, 9, 1, 0),
    device_id: beta,
    sentence: "Ran a command",
    tool: "runs.exec",
    caller: "client"
  });

  it("labels the machine chips and the caller control, and counts the filter's own window", () => {
    // The filter's count is the rows it would draw from the fetched window, not the day's
    // `client calls` number. The noun phrase on the page's header stays the only one with a window.
    render(<ActivityTimeline lines={[clientAlpha, clientBeta]} devices={devices} />);

    expect(screen.getByText("Machines")).toBeInTheDocument();
    expect(screen.getByText("Callers")).toBeInTheDocument();
    const machines = screen.getByRole("group", { name: "Filter by machine" });
    expect(within(machines).getByRole("button", { name: "Alpha" })).toBeInTheDocument();
    expect(within(machines).getByRole("button", { name: "All machines" })).toBeInTheDocument();

    const chip = screen.getByRole("button", { name: "Clients only" });
    expect(chip).toHaveTextContent(/^Clients only$/);
    expect(chip).not.toHaveTextContent(/client calls/i);
    expect(chip).toHaveAccessibleDescription("2 client calls in this window · from 09:00, spanning 1 m");

    // The count follows the machine filter: it is the subset the control would draw from this
    // machine's window. **AND IT NAMES THAT SET (the thirty-seventh list).** With a scope active the
    // sentence says which machine the count is of, and it withholds the span — the span belongs to
    // the whole read, so stating it beside a narrowed count joined two different measurements.
    fireEvent.click(within(machines).getByRole("button", { name: "Beta" }));
    expect(screen.getByRole("button", { name: "Clients only" })).toHaveAccessibleDescription(
      "1 client call on Beta in this window"
    );
  });

  it("names the set a narrowed count is of, and withholds the span that belongs to the read", () => {
    // **MEASURED AFTER A SEARCH THAT MATCHED NOTHING (the thirty-seventh list).** The page stated
    // `0 client calls in this window · spanning 10 h 50 m`: the `0` was the SEARCH-FILTERED
    // client-call count and the span was the UNFILTERED window's, joined by a `·` that read as "the
    // span of the zero" — under a head that still said `Today: 876 client calls`. Three numbers,
    // three different sets, no sign of which was which.
    // A client call is one whose caller says so or one that carries a token; both are needed for the
    // count to be non-zero before the search narrows it.
    const newest = line({
      ts_ms: Date.UTC(2026, 10, 5, 9, 1, 0),
      tool: "terminal.list",
      device_id: alpha,
      caller: "client"
    });
    const older = line({
      ts_ms: Date.UTC(2026, 10, 5, 9, 0, 0),
      tool: "terminal.list",
      device_id: beta,
      caller: "client"
    });
    render(
      <ActivityTimeline
        lines={[newest, older]}
        devices={devices}
        nowMs={Date.UTC(2026, 10, 5, 10, 0, 0)}
      />
    );

    const chip = screen.getByRole("button", { name: "Clients only" });
    // Unfiltered: the window's own count and its own span, as before.
    expect(chip).toHaveAccessibleDescription("2 client calls in this window · from 09:00, spanning 1 m");

    const search = screen.getByRole("searchbox", { name: "Search activity" });
    fireEvent.change(search, { target: { value: "no-such-line" } });
    // Narrowed: the sentence names the set and the span is gone — the span belongs to the read, and
    // the read is not what the reader is looking at.
    expect(screen.getByRole("button", { name: "Clients only" })).toHaveAccessibleDescription(
      "0 client calls matching the search in this window"
    );
    expect(screen.getByRole("button", { name: "Clients only" }).getAttribute("aria-describedby")).toBe(
      "timeline-clients-window"
    );
    // And clearing the search puts both back.
    fireEvent.change(search, { target: { value: "" } });
    expect(screen.getByRole("button", { name: "Clients only" })).toHaveAccessibleDescription(
      "2 client calls in this window · from 09:00, spanning 1 m"
    );
  });

  it("groups its days at the midnight the count used, not the browser's (the forty-fourth list)", () => {
    // **ONE INTERVAL NAME OVER TWO INTERVALS.** Measured from a browser set to `America/New_York`
    // (midnight 05:00Z): the page drew the `Today` label over rows from 07:48Z while the day line
    // above it had counted from 00:00Z. The server now publishes `day_start_ms`, and the labels cut
    // there — so a row at 01:00Z is TODAY when the cut is 00:00Z even in a browser whose own day
    // began at 05:00Z. The test sets the cut explicitly, which is what a page does with the answer.
    const cut = Date.UTC(2026, 10, 5, 0, 0, 0);
    const afterCut = line({
      ts_ms: cut + 3_600_000,
      device_id: alpha,
      sentence: "Read notes.txt",
      tool: "files.read",
      caller: "client"
    });
    const beforeCut = line({
      ts_ms: cut - 3_600_000,
      device_id: alpha,
      sentence: "Read notes.txt",
      tool: "files.read",
      caller: "client"
    });
    render(
      <ActivityTimeline
        lines={[afterCut, beforeCut]}
        devices={devices}
        dayStartMs={cut}
        nowMs={cut + 7_200_000}
      />
    );
    // The two rows are in DIFFERENT days, because the cut is the boundary — with the browser's own
    // midnight both would fall in one day on a UTC host and in two on a New York one.
    expect(screen.getByText("Today")).toBeInTheDocument();
    expect(screen.getByText("Yesterday")).toBeInTheDocument();
  });

  it("names the fetched window's span beside the rows it counts", () => {
    // The measured Activity page put `Today: 18 client calls` over `486 rows in this window`; the
    // window was 24 h 37 m and the span was stated nowhere, so the two counts read as a
    // contradiction. The filter's own count now names the window it counted.
    const newest = line({
      ts_ms: Date.UTC(2026, 10, 5, 9, 0, 0),
      device_id: alpha,
      sentence: "Read notes.txt",
      tool: "files.read",
      caller: "client"
    });
    const oldest = line({
      ts_ms: Date.UTC(2026, 10, 4, 8, 23, 0),
      device_id: alpha,
      sentence: "Listed the terminal sessions",
      tool: "terminal.list",
      caller: "console"
    });
    render(<ActivityTimeline lines={[newest, oldest]} devices={devices} />);

    // **AND IT SAYS WHERE THE WINDOW STARTS (the forty-third list).** Measured live: the header read
    // `Today: 206 client calls` over `629 client calls in this window · spanning 12 h 15 m`, and both
    // were true — 629 > 206 because a 12 h 15 m window read at 19:48 starts at 07:33 and never crosses
    // midnight. A span alone cannot say that; the START can, and it is the one fact that reconciles
    // the two numbers a reader is looking at together.
    expect(screen.getByRole("button", { name: "Clients only" })).toHaveAccessibleDescription(
      "1 client call in this window · from 08:23, spanning 24 h 37 m"
    );
  });

  it("puts the window and source hints after the timeline, not above it", () => {
    // The measured page put two gray technical lines between the filters and the day. The record
    // now ends with them: a reader meets the day first, and consults what was drawn/read after it.
    const source: AuditSource = {
      file: "audit.jsonl",
      bytes: 5_872_026,
      oldest_ts_ms: Date.UTC(2026, 9, 22, 10, 55),
      newest_ts_ms: Date.UTC(2026, 10, 5, 9, 0),
      max_bytes: 8_388_608,
      rotations: 0
    };
    render(<ActivityTimeline lines={[clientAlpha]} devices={devices} atLimit source={source} />);

    const timeline = document.querySelector<HTMLElement>(".timeline");
    const footer = document.querySelector<HTMLElement>(".timeline-footer");
    const capped = document.querySelector<HTMLElement>(".timeline-capped");
    expect(timeline).not.toBeNull();
    expect(footer).not.toBeNull();
    expect(
      (timeline as HTMLElement).compareDocumentPosition(footer as HTMLElement) &
        Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    // The cap sentence qualifies the count at the top of the record, not the footer: it moved above
    // the first day label where the rows it counts begin (backlog item 2).
    expect(timeline?.querySelector(".timeline-capped")).not.toBeNull();
    expect(timeline?.querySelector(".audit-source")).toBeNull();
    expect(capped).toHaveTextContent("Showing the newest 1000 lines — the window's limit.");
    expect(footer?.querySelector(".audit-source")).toHaveTextContent(/^Reading audit\.jsonl/);
  });
});

/**
 * THE PHONE'S FILTER BLOCK IS THE RECORD'S FOE. At 390 the two dated `old id` chips took two full
 * rows above the first record, which started at y=600 — below the fold of the page's own head and
 * filters (audit-40 F5). They are already options in the Table's Device filter, so the phone keeps
 * the live machines and the ACTIVE scope; the desktop keeps every dated chip.
 */
describe("ActivityTimeline on a phone", () => {
  const matchMediaDescriptor = Object.getOwnPropertyDescriptor(window, "matchMedia");

  beforeEach(() => {
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: vi.fn(() => ({
        matches: true,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn()
      }))
    });
  });

  afterEach(() => {
    restoreGlobal(window, "matchMedia", matchMediaDescriptor);
  });

  it("drops the retired ids from the machine chips, keeping the live machines", () => {
    const ghost = line({
      ts_ms: Date.UTC(2026, 10, 5, 9, 5, 0),
      device_id: retiredHex,
      sentence: "Listed the plugins",
      tool: "plugins.list"
    });
    render(
      <ActivityTimeline
        lines={[ghost, fromAlpha]}
        devices={devices}
        nowMs={Date.UTC(2026, 10, 5, 9, 10, 0)}
      />
    );

    const group = screen.getByRole("group", { name: "Filter by machine" });
    expect(within(group).getByRole("button", { name: "All machines" })).toBeInTheDocument();
    expect(within(group).getByRole("button", { name: "Alpha" })).toBeInTheDocument();
    expect(within(group).getByRole("button", { name: "Beta" })).toBeInTheDocument();
    expect(within(group).queryByText(/old id/)).toBeNull();
    // The row is still the record; only its scope chip is gone (the Table's Device filter has it).
    expect(screen.getByText("Listed the plugins")).toBeInTheDocument();
  });

  it("keeps an active retired scope visible, because a hidden filter is a trap", () => {
    render(
      <ActivityTimeline
        lines={[fromAlpha]}
        devices={devices}
        device={retiredHex}
        nowMs={Date.UTC(2026, 10, 5, 9, 10, 0)}
      />
    );

    const group = screen.getByRole("group", { name: "Filter by machine" });
    const chip = within(group).getByRole("button", { name: "old id · fe209fc2c119…" });
    expect(chip).toHaveAttribute("aria-pressed", "true");
    expect(within(group).queryByRole("button", { name: "Alpha" })).toBeInTheDocument();
  });
});

/**
 * A DAY IS ITS NEWEST ROWS, THEN ONE DOOR (backlog item 2). On the operator's page 692 rows sat
 * under `Today` and `main` was 73,740 px tall at 1440; the day's count and span were at the bottom
 * of the wall. A day is drawn as sixty rows and one muted door that states the rest.
 */
describe("ActivityTimeline's day door", () => {
  /** One day's rows, newest first, each a distinct second so every row has its own key. */
  function dayRows(count: number, dayOffset = 0): AuditLine[] {
    const newest = Date.UTC(2026, 10, 5 - dayOffset, 12, 43, 0);
    return Array.from({ length: count }, (_, index) =>
      line({
        ts_ms: newest - index * 1_000,
        device_id: alpha,
        sentence: `Call ${index + 1}`,
        tool: "files.read",
        caller: "client"
      })
    );
  }

  it("draws 60 rows and one door naming the rest, with the day's count and span", () => {
    render(
      <ActivityTimeline
        lines={dayRows(200)}
        devices={devices}
        nowMs={Date.UTC(2026, 10, 5, 12, 50, 0)}
      />
    );

    expect(document.querySelectorAll(".timeline-row")).toHaveLength(60);
    const doors = document.querySelectorAll(".timeline-more");
    expect(doors).toHaveLength(1);
    // ONE sentence, the day's own label word, the day's own count of what is behind it.
    expect(doors[0]).toHaveTextContent("Show the rest of today (140 more)");
    // And the day it belongs to, in the page's vocabulary: the stretch label's noun and the
    // provenance line's clock, over the whole day rather than only the drawn rows.
    expect(doors[0]).toHaveTextContent("200 calls · 12:39 → 12:43");
    expect(screen.queryByText("Call 140")).toBeNull();
    expect(screen.getByText("Call 60")).toBeInTheDocument();
  });

  it("opens the day in place when the door is pressed, and the door goes", () => {
    render(
      <ActivityTimeline
        lines={dayRows(200)}
        devices={devices}
        nowMs={Date.UTC(2026, 10, 5, 12, 50, 0)}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: /Show the rest of today/ }));

    expect(document.querySelectorAll(".timeline-row")).toHaveLength(200);
    expect(screen.queryByRole("button", { name: /Show the rest of today/ })).toBeNull();
    expect(document.querySelector(".timeline-more")).toBeNull();
    expect(screen.getByText("Call 200")).toBeInTheDocument();
  });

  it("draws no door for a day of five rows", () => {
    render(
      <ActivityTimeline
        lines={dayRows(5)}
        devices={devices}
        nowMs={Date.UTC(2026, 10, 5, 12, 50, 0)}
      />
    );

    expect(document.querySelectorAll(".timeline-row")).toHaveLength(5);
    expect(document.querySelector(".timeline-more")).toBeNull();
    expect(screen.queryByRole("button", { name: /Show the rest/ })).toBeNull();
  });

  it("puts the cap sentence above the first day label, where the count it qualifies is", () => {
    render(
      <ActivityTimeline
        lines={dayRows(80)}
        devices={devices}
        capped
        nowMs={Date.UTC(2026, 10, 5, 12, 50, 0)}
      />
    );

    const capped = document.querySelector<HTMLElement>(".timeline-capped");
    const firstLabel = document.querySelector<HTMLElement>(".timeline-day-label");
    expect(capped).not.toBeNull();
    expect(firstLabel).not.toBeNull();
    expect(
      (capped as HTMLElement).compareDocumentPosition(firstLabel as HTMLElement) &
        Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    // The day label is inside the timeline, so the sentence is above the record and not below it.
    const before = document.querySelector<HTMLElement>(".timeline-day-label");
    expect(before?.previousElementSibling).toBe(capped);
  });
});
