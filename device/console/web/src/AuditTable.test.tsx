import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditLine, Device, TokenMeta } from "./api";
import { AuditTable } from "./AuditTable";
import { downloadFile } from "./download";

vi.mock("./download", () => ({ downloadFile: vi.fn() }));

/** The part of a fetch Response `api.auditExport` reads, without a real network or a DOM Response. */
function exportAnswer(body: string, headers: Record<string, string>, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText: status === 200 ? "OK" : "Payload Too Large",
    text: async () => body,
    headers: { get: (name: string) => headers[name.toLowerCase()] ?? null }
  } as Response;
}

/** The sentence a range too large to export gets back from the console's 413 answer. */
const refusalMessage =
  "the range needs more than the 50,000-line scan bound; ask for a narrower range";

/** The console's refusal of a range the scan bound cannot prove, as `api.auditExport` reads one. */
function refusedExport(message: string): Response {
  return exportAnswer(JSON.stringify({ error: "range_too_large", message }), {
    "content-type": "application/json"
  }, 413);
}

const alpha = "device-a-0123456789ab";
const beta = "device-b-0123456789ab";

/** One drawn range, used by the export refusal cases. */
const drawnRange = {
  from: new Date(2026, 10, 5, 9, 0, 0, 0).getTime(),
  to: new Date(2026, 10, 5, 17, 0, 0, 0).getTime()
};

function line(overrides: Partial<AuditLine> = {}): AuditLine {
  return {
    ts_ms: Date.UTC(2026, 9, 26, 8, 0, 0),
    kind: "denied",
    device_id: null,
    tool: null,
    detail: null,
    status: 401,
    ...overrides
  };
}

const devices: Device[] = [
  { id: alpha, name: "Alpha", online: true, version: "1.2.3", last_seen_ms: 1 },
  { id: beta, name: "Beta", online: false, version: null, last_seen_ms: null }
];

const lines: AuditLine[] = [
  {
    ts_ms: Date.UTC(2026, 9, 26, 8, 0, 0),
    kind: "tool",
    device_id: alpha,
    tool: "memory.export",
    detail: "wrote 3 entries",
    status: 200
  },
  {
    ts_ms: Date.UTC(2026, 9, 26, 8, 0, 1),
    kind: "tool",
    device_id: beta,
    tool: "terminal.write",
    detail: null,
    status: 403
  },
  {
    ts_ms: Date.UTC(2026, 9, 26, 8, 0, 2),
    kind: "tool",
    device_id: alpha,
    tool: "terminal.list",
    detail: null,
    status: 200
  }
];

const auditRows = () => Array.from(document.querySelectorAll<HTMLElement>("tr.audit-row"));

/** The table row that carries a cell with this text (filters and records may show it too). */
function rowOf(text: string): HTMLElement {
  const row = auditRows().find((entry) => (entry.textContent ?? "").includes(text));
  expect(row).toBeDefined();
  return row as HTMLElement;
}

/** The row's one control: its disclosure button. */
function disclosureOf(row: HTMLElement): HTMLElement {
  return within(row).getByRole("button", { name: /the record for/ });
}

/** One click on the row's disclosure opens its record, a second closes it. */
function toggleRow(text: string): HTMLElement {
  const button = disclosureOf(rowOf(text));
  fireEvent.click(button);
  return button;
}

/** The record `<tr>` that follows a row once its disclosure was opened. */
function recordOf(text: string): HTMLElement {
  const record = rowOf(text).nextElementSibling;
  expect(record).toHaveClass("audit-record-row");
  return record as HTMLElement;
}

it("says so when the window it was given was cut", () => {
  // The table cannot know more than the lines it was handed; when the fetch came back full, "All
  // time" is a window, and saying nothing would present it as the whole history.
  const full: AuditLine[] = Array.from({ length: 1000 }, (_, index) => ({
    ...lines[0],
    ts_ms: lines[0].ts_ms - index * 1000,
    tool: `tool.${index}`
  }));
  const { rerender } = render(<AuditTable lines={lines} devices={devices} tokens={[]} capped />);
  expect(
    screen.getByText("Showing the newest 1000 lines; older ones are not on this page.")
  ).toBeInTheDocument();

  rerender(<AuditTable lines={lines} devices={devices} tokens={[]} />);
  expect(
    screen.queryByText("Showing the newest 1000 lines; older ones are not on this page.")
  ).not.toBeInTheDocument();
  expect(full).toHaveLength(1000);
});

it("names the machine whose window was cut", () => {
  render(<AuditTable lines={lines} devices={devices} tokens={[]} capped cappedMachine="Alpha" />);
  expect(
    screen.getByText("Showing the newest 1000 lines from Alpha; older ones are not on this page.")
  ).toBeInTheDocument();
});

it("says the window's limit when the request came back full", () => {
  const { rerender } = render(<AuditTable lines={lines} devices={devices} tokens={[]} />);
  expect(screen.queryByText(/the window's limit/)).toBeNull();

  // The server answered with exactly the limit. The page does not know an older line exists and
  // must not say one does; it names the window instead.
  rerender(<AuditTable lines={lines} devices={devices} tokens={[]} atLimit />);
  expect(
    screen.getByText("Showing the newest 1000 lines — the window's limit.")
  ).toBeInTheDocument();
});

it("counts the rows the table draws, which include the channel boundaries the timeline hides", () => {
  // The table is the audit: it draws the `device.left`/`device.joined` lines the timeline drops, so
  // its sentence names the 983 merged rows, not the timeline's 949, and only the repeats withheld.
  render(
    <AuditTable
      lines={lines}
      devices={devices}
      tokens={[]}
      atLimit
      windowCounts={{ fetched: 1000, drawn: 983, boundaries: 0, repeats: 17 }}
    />
  );

  expect(
    screen.getByText(
      "Showing the newest 983 lines of 1000 fetched (17 repeats not drawn)."
    )
  ).toBeInTheDocument();
});

it("reads the new window in one line instead of drawing the old window's rows", () => {
  render(
    <AuditTable
      lines={lines}
      devices={devices}
      tokens={[]}
      selectedDeviceId={alpha}
      loading
      loadingMachine="Alpha"
    />
  );

  expect(screen.getByText("Reading Alpha's window…")).toBeInTheDocument();
  expect(screen.queryByText("memory.export")).toBeNull();
});

it("makes the row's opener a real button with aria-expanded", () => {
  render(<AuditTable lines={lines} devices={devices} tokens={[]} />);
  const row = rowOf("memory.export");
  expect(row).not.toHaveAttribute("tabindex");
  const button = disclosureOf(row);
  expect(button.tagName).toBe("BUTTON");
  expect(button).toHaveAttribute("type", "button");
  expect(button).toHaveAttribute("aria-expanded", "false");

  fireEvent.click(button);

  expect(button).toHaveAttribute("aria-expanded", "true");
  expect(document.querySelector(".audit-record")).not.toBeNull();
});

// Two record ids as the console writes them: 64 lowercase hex, never the `srm_` secret.
const tokenA = "a".repeat(64);
const tokenB = "b".repeat(64);
const shortA = "a".repeat(12) + "…";
const shortB = "b".repeat(12) + "…";

// The operator's list, as `GET /tokens` answers it: metadata only, so the name is the only thing the
// table can get from here. A token the list does not know (revoked, or created in another tab) has
// nothing but its id.
const tokens: TokenMeta[] = [
  {
    id: tokenA,
    name: "laptop-dsh",
    device_id: alpha,
    tools: ["files.read"],
    allow_destructive: false,
    created_at_ms: 1
  },
  {
    id: tokenB,
    name: "spare-dsh",
    device_id: beta,
    tools: ["host.facts"],
    allow_destructive: false,
    created_at_ms: 2
  }
];

describe("AuditTable", () => {
  beforeEach(() => {
    vi.mocked(downloadFile).mockReset();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("shows a relative time and keeps the exact date in the title", () => {
    const ts = Date.now() - 3 * 24 * 60 * 60_000;
    render(
      <AuditTable lines={[line({ ts_ms: ts, device_id: alpha, status: 200 })]} devices={devices} tokens={[]} />
    );

    const cell = screen.getByTitle(new Date(ts).toISOString());
    expect(cell.textContent).toMatch(/ago|just now/);
    expect(cell.textContent).toMatch(/\d+h ago/);
  });

  it("groups the record by day, with one label for the day's lines", () => {
    render(<AuditTable lines={lines} devices={devices} tokens={[]} />);

    const days = document.querySelectorAll("tr.audit-day th");
    expect(days).toHaveLength(1);
    expect(days[0].textContent).toMatch(/2026|Today|Yesterday/);
  });

  it("exports the filtered view as CSV from the overflow", () => {
    render(<AuditTable lines={lines} devices={devices} tokens={[]} />);

    fireEvent.click(screen.getByRole("button", { name: "Export CSV" }));

    expect(downloadFile).toHaveBeenCalledTimes(1);
    const [name, body, type] = vi.mocked(downloadFile).mock.calls[0];
    expect(name).toMatch(/^summrise-audit-\d{8}-\d{6}\.csv$/);
    expect(body).toContain('"tool"');
    expect(type).toBe("text/csv;charset=utf-8");
  });

  it("exports the newest window in hand as JSON Lines, oldest first", () => {
    render(<AuditTable lines={lines} devices={devices} tokens={[]} />);

    const button = screen.getByRole("button", {
      name: /^Export JSONL: the newest window in hand/
    });
    // The visible word names the format, so it is not mistaken for the CSV beside it; the
    // accessible name still says the file is the window, not the table's narrowed rows.
    expect(button).toHaveTextContent("Export JSONL");
    expect(button).toHaveAccessibleName(/the table's other filters do not apply$/);
    fireEvent.click(button);

    expect(downloadFile).toHaveBeenCalledTimes(1);
    const [name, body, type] = vi.mocked(downloadFile).mock.calls[0];
    expect(name).toBe("summrise-audit-20261026T080000.000Z_to_20261026T080002.001Z.ndjson");
    expect(type).toBe("application/x-ndjson");
    const written = String(body)
      .trimEnd()
      .split("\n")
      .map((entry) => JSON.parse(entry));
    expect(written.map((entry) => entry.ts_ms)).toEqual([
      lines[0].ts_ms,
      lines[1].ts_ms,
      lines[2].ts_ms
    ]);
    expect(String(body).startsWith("{")).toBe(true);
  });

  it("leaves both exports disabled when the drawn range held no lines", () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    render(<AuditTable lines={[]} devices={devices} tokens={[]} range={drawnRange} />);

    // The range's own sentence says it is empty; both controls must agree with it rather than
    // offer a press that could only download a zero-byte file.
    expect(screen.getByText(/0 lines/)).toBeInTheDocument();
    const jsonl = screen.getByRole("button", { name: /^Export JSONL: the drawn range/ });
    const csv = screen.getByRole("button", { name: "Export CSV" });
    expect(jsonl).toBeDisabled();
    expect(csv).toBeDisabled();

    fireEvent.click(jsonl);
    fireEvent.click(csv);
    expect(fetchMock).not.toHaveBeenCalled();
    expect(downloadFile).not.toHaveBeenCalled();
  });

  it("keeps both exports when the drawn range held lines", () => {
    render(<AuditTable lines={lines} devices={devices} tokens={[]} range={drawnRange} />);

    expect(screen.getByRole("button", { name: /^Export JSONL: the drawn range/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Export CSV" })).toBeEnabled();
  });

  it("asks the console for the drawn range, device and bounds included", async () => {
    const range = {
      from: new Date(2026, 10, 5, 9, 0, 0, 0).getTime(),
      to: new Date(2026, 10, 5, 17, 0, 0, 0).getTime()
    };
    const filename = "summrise-audit-20261105T010000.000Z_to_20261105T090000.000Z_a1a1a1a1a1a1.ndjson";
    const fetchMock = vi.fn().mockResolvedValue(
      exportAnswer('{"ts_ms":1,"kind":"mcp"}\n', {
        "content-type": "application/x-ndjson",
        "content-disposition": `attachment; filename="${filename}"`
      })
    );
    vi.stubGlobal("fetch", fetchMock);

    render(
      <AuditTable
        lines={lines}
        devices={devices}
        tokens={[]}
        selectedDeviceId={alpha}
        range={range}
      />
    );
    fireEvent.click(
      screen.getByRole("button", { name: /^Export JSONL: the drawn range/ })
    );

    await waitFor(() => expect(downloadFile).toHaveBeenCalledTimes(1));
    expect(fetchMock).toHaveBeenCalledWith(
      `/audit/export?from=${range.from}&to=${range.to}&device=${alpha}`
    );
    const [name, body, type] = vi.mocked(downloadFile).mock.calls[0];
    expect(name).toBe(filename);
    expect(String(body)).toBe('{"ts_ms":1,"kind":"mcp"}\n');
    expect(type).toBe("application/x-ndjson");
  });

  it("says a range too large to export was refused, and saves no partial file", async () => {
    const range = {
      from: new Date(2026, 10, 5, 9, 0, 0, 0).getTime(),
      to: new Date(2026, 10, 5, 17, 0, 0, 0).getTime()
    };
    const fetchMock = vi.fn().mockResolvedValue(refusedExport(refusalMessage));
    vi.stubGlobal("fetch", fetchMock);

    render(<AuditTable lines={lines} devices={devices} tokens={[]} range={range} />);
    fireEvent.click(
      screen.getByRole("button", { name: /^Export JSONL: the drawn range/ })
    );

    expect(await screen.findByRole("alert")).toHaveTextContent("50,000-line scan bound");
    expect(downloadFile).not.toHaveBeenCalled();
  });

  it("draws an export's refusal as an inline alert with a Dismiss control", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(refusedExport(refusalMessage)));

    render(<AuditTable lines={lines} devices={devices} tokens={[]} range={drawnRange} />);
    fireEvent.click(
      screen.getByRole("button", { name: /^Export JSONL: the drawn range/ })
    );

    const alert = await screen.findByRole("alert");
    // The route's sentence is verbatim; only the surface it lands on changed.
    expect(alert).toHaveTextContent(`The console refused this export: ${refusalMessage}`);
    expect(alert).toHaveClass("alert-inline");
    expect(alert).not.toHaveClass("hint");
    expect(within(alert).getByRole("button", { name: "Dismiss" })).toBeInTheDocument();
  });

  it("forgets an export's refusal as soon as the range changes", async () => {
    const onRange = vi.fn();
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(refusedExport(refusalMessage)));
    const { rerender } = render(
      <AuditTable
        lines={lines}
        devices={devices}
        tokens={[]}
        range={drawnRange}
        onRange={onRange}
      />
    );
    fireEvent.click(
      screen.getByRole("button", { name: /^Export JSONL: the drawn range/ })
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("50,000-line scan bound");

    fireEvent.change(screen.getByLabelText("Time"), { target: { value: "hour" } });

    // The parent has not answered with the new range's rows yet. The old sentence describes a range
    // the operator has already left, so it goes with the control's change, not with the answer.
    expect(onRange).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("alert")).toBeNull();
    rerender(
      <AuditTable
        lines={lines}
        devices={devices}
        tokens={[]}
        range={onRange.mock.calls[0][0]}
        onRange={onRange}
      />
    );
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("dismisses an export's refusal", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(refusedExport(refusalMessage)));

    render(<AuditTable lines={lines} devices={devices} tokens={[]} range={drawnRange} />);
    fireEvent.click(
      screen.getByRole("button", { name: /^Export JSONL: the drawn range/ })
    );

    const alert = await screen.findByRole("alert");
    fireEvent.click(within(alert).getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("forgets an export's refusal when a later export succeeds", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(refusedExport(refusalMessage))
      .mockResolvedValueOnce(
        exportAnswer('{"ts_ms":1,"kind":"mcp"}\n', {
          "content-type": "application/x-ndjson",
          "content-disposition": 'attachment; filename="summrise-audit.ndjson"'
        })
      );
    vi.stubGlobal("fetch", fetchMock);

    render(<AuditTable lines={lines} devices={devices} tokens={[]} range={drawnRange} />);
    const button = screen.getByRole("button", {
      name: /^Export JSONL: the drawn range/
    });
    fireEvent.click(button);
    expect(await screen.findByRole("alert")).toHaveTextContent("50,000-line scan bound");

    fireEvent.click(button);

    await waitFor(() => expect(downloadFile).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  it("does not draw a refusal that lands after the range it answered for is gone", async () => {
    let refuse!: (answer: Response) => void;
    const answer = new Promise<Response>((resolve) => {
      refuse = resolve;
    });
    const fetchMock = vi.fn().mockReturnValue(answer);
    vi.stubGlobal("fetch", fetchMock);
    const onRange = vi.fn();
    const { rerender } = render(
      <AuditTable
        lines={lines}
        devices={devices}
        tokens={[]}
        range={drawnRange}
        onRange={onRange}
      />
    );
    fireEvent.click(
      screen.getByRole("button", { name: /^Export JSONL: the drawn range/ })
    );
    fireEvent.change(screen.getByLabelText("Time"), { target: { value: "hour" } });
    rerender(
      <AuditTable
        lines={lines}
        devices={devices}
        tokens={[]}
        range={onRange.mock.calls[0][0]}
        onRange={onRange}
      />
    );

    refuse(refusedExport(refusalMessage));

    // The request belonged to the old range. Let its refusal arrive and the catch run; the alert
    // must still not describe a range the page no longer shows.
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("asks the console for a preset instead of filtering the window it holds", () => {
    // The Time control used to cut the 1,000-line window in the browser and fire nothing. The
    // window is the server's answer now: a preset is a request, and the rows it holds are not
    // filtered locally — the page draws whatever the range's answer carries.
    const onRange = vi.fn();
    render(
      <AuditTable
        lines={[
          line({ ts_ms: Date.now() - 2 * 24 * 60 * 60_000, tool: "old.tool", status: 200 }),
          line({ ts_ms: Date.now() - 60_000, tool: "recent.tool", status: 200 })
        ]}
        devices={devices}
        tokens={[]}
        range={null}
        onRange={onRange}
      />
    );
    expect(screen.getByLabelText("Time")).toHaveValue("window");
    expect(screen.getByRole("option", { name: "Newest window" })).toBeInTheDocument();
    expect(screen.queryByText(/^Showing .*lines?$/)).toBeNull();
    expect(screen.getByText("old.tool")).toBeInTheDocument();

    const before = Date.now();
    fireEvent.change(screen.getByLabelText("Time"), { target: { value: "hour" } });
    const after = Date.now();

    expect(onRange).toHaveBeenCalledTimes(1);
    const chosen = onRange.mock.calls[0][0];
    expect(chosen.to - chosen.from).toBe(60 * 60_000);
    expect(chosen.to).toBeGreaterThanOrEqual(before);
    expect(chosen.to).toBeLessThanOrEqual(after);
    // Nothing was filtered in the browser: the preset is an address for a different read.
    expect(screen.getByText("old.tool")).toBeInTheDocument();
  });

  it("issues a custom from and to", () => {
    const onRange = vi.fn();
    render(
      <AuditTable lines={lines} devices={devices} tokens={[]} range={null} onRange={onRange} />
    );

    fireEvent.change(screen.getByLabelText("Time"), { target: { value: "custom" } });
    fireEvent.change(screen.getByLabelText("From"), { target: { value: "2026-11-05T09:00" } });
    fireEvent.change(screen.getByLabelText("To"), { target: { value: "2026-11-05T17:00" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    expect(onRange).toHaveBeenCalledWith({
      from: new Date(2026, 10, 5, 9, 0, 0, 0).getTime(),
      to: new Date(2026, 10, 5, 17, 0, 0, 0).getTime()
    });
  });

  it("names the range it drew, and goes back to the newest window", () => {
    const range = {
      from: new Date(2026, 10, 5, 9, 0, 0, 0).getTime(),
      to: new Date(2026, 10, 5, 17, 0, 0, 0).getTime()
    };
    const onRange = vi.fn();
    const { rerender } = render(
      <AuditTable
        lines={lines}
        devices={devices}
        tokens={[]}
        range={range}
        onRange={onRange}
      />
    );

    expect(
      screen.getByText("Showing 09:00–17:00 of 11-05, 3 lines")
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Time")).toHaveValue("custom");

    fireEvent.change(screen.getByLabelText("Time"), { target: { value: "window" } });
    expect(onRange).toHaveBeenCalledWith(null);

    // The page clears the range and the sentence goes with it; the newest window is named by the
    // control, not by a cap sentence it has no reason to show.
    rerender(<AuditTable lines={lines} devices={devices} tokens={[]} range={null} onRange={onRange} />);
    expect(screen.queryByText(/^Showing 09:00/)).not.toBeInTheDocument();
    expect(screen.getByLabelText("Time")).toHaveValue("window");
  });

  it("states a range's missing start and the scan bound that cut it", () => {
    const range = {
      from: new Date(2026, 10, 5, 9, 0, 0, 0).getTime(),
      to: new Date(2026, 10, 5, 17, 0, 0, 0).getTime()
    };
    render(
      <AuditTable
        lines={lines}
        devices={devices}
        tokens={[]}
        range={range}
        rangeFacts={{
          since_ms: new Date(2026, 9, 22, 10, 55, 0, 0).getTime(),
          bounded: true,
          scan_limit: 50_000
        }}
      />
    );

    const sentence = screen.getByText(/^Showing 09:00/).textContent ?? "";
    expect(sentence).toContain("since 10-22 10:55");
    expect(sentence).toContain("the 50,000-line scan bound cut it");
  });

  it("opens scoped to the selected machine and can widen to the fleet", () => {
    render(<AuditTable lines={lines} devices={devices} tokens={[]} selectedDeviceId={alpha} />);

    expect(screen.getByRole("button", { name: "This machine" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByText("terminal.write")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "All machines" }));

    expect(screen.getByRole("button", { name: "All machines" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("terminal.write")).toBeInTheDocument();
  });

  it("opens a run's record from the whole row and follows it to Runs", () => {
    const onOpenRun = vi.fn();
    const run = line({
      kind: "mcp",
      device_id: alpha,
      tool: "runs.exec",
      detail: "run r-9 exited 0 in 42 ms",
      status: 200
    });
    render(
      <AuditTable lines={[run]} devices={devices} tokens={[]} onOpenRun={onOpenRun} />
    );
    fireEvent.click(disclosureOf(rowOf("runs.exec")));
    fireEvent.click(screen.getByRole("button", { name: "Open in Runs" }));

    expect(onOpenRun).toHaveBeenCalledWith(alpha, "r-9");
  });

  it("names the device in the row and keeps the full id for the record", () => {
    render(<AuditTable lines={lines} devices={devices} tokens={[]} />);

    const row = rowOf("memory.export") as HTMLTableRowElement;
    const cell = row.cells[1];
    expect(cell).toHaveTextContent("Alpha");
    // The title still names the id, which the record's Copy id is the one control for.
    expect(cell).toHaveAttribute("title", expect.stringContaining(alpha));
    expect(within(row).getByText("memory.export")).toBeInTheDocument();
    // No KIND column: `mcp` on every row is not information.
    expect(screen.queryByRole("columnheader", { name: /kind/i })).not.toBeInTheDocument();
  });

  it("shows the distinguishing tail of a machine's name on a phone, named whole to a screen reader", () => {
    // The measured phone: a 67 px device cell held "DESKTOP-9VH92BJ console" in three lines and
    // grew the row to 91 px. The cell keeps the whole name as its accessible name and in its
    // title; what is drawn is the part the three machines do not share.
    const desktops: Device[] = [
      { id: "desk-1", name: "DESKTOP-9VH92BJ", online: true, version: "1.0", last_seen_ms: 1 },
      { id: "desk-2", name: "DESKTOP-14RJCR8", online: true, version: "1.0", last_seen_ms: 1 },
      { id: "desk-3", name: "DESKTOP-CPH4KB2", online: true, version: "1.0", last_seen_ms: 1 }
    ];
    render(
      <AuditTable
        lines={[line({ device_id: "desk-1", tool: "terminal.list", status: 200 })]}
        devices={desktops}
        tokens={[]}
      />
    );

    const row = document.querySelector("tr.audit-row") as HTMLTableRowElement;
    const cell = row.cells[1];
    expect(cell).toHaveAccessibleName("DESKTOP-9VH92BJ");
    expect(cell).toHaveAttribute("title", expect.stringContaining("DESKTOP-9VH92BJ"));
    // 7 characters, not the 15-character name that broke into three lines.
    expect(cell.querySelector(".audit-device-short")).toHaveTextContent("9VH92BJ");
  });

  it("puts the row's opener in its first cell, so a phone does not swipe to open it", () => {
    // <=639 px: the sixth column is not rendered and the disclosure moves into the When cell. The
    // assertion is structural (the first `td`, the row's first focusable element), not a
    // `matchMedia` reading: jsdom has no layout, and the media query is proven by the screenshots.
    render(<AuditTable lines={lines} devices={devices} tokens={[]} />);

    const row = rowOf("memory.export") as HTMLTableRowElement;
    const firstCell = row.cells[0];
    const disclosure = firstCell.querySelector<HTMLButtonElement>("button.audit-disclosure");
    expect(disclosure).not.toBeNull();
    expect(firstCell.querySelector("button")).toBe(disclosure);
    expect(disclosure).toHaveAttribute("aria-expanded", "false");
    expect(disclosure).toHaveAccessibleName("Show the record for memory.export");

    const focusable = row.querySelectorAll<HTMLElement>(
      'button, a[href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
    );
    expect(focusable[0]).toBe(disclosure);

    fireEvent.click(disclosure as HTMLButtonElement);
    expect(disclosure).toHaveAttribute("aria-expanded", "true");
    expect(recordOf("memory.export")).toBeInTheDocument();
  });

  it("copies the full device id from an open line without collapsing it", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    const original = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    try {
      render(<AuditTable lines={lines} devices={devices} tokens={[]} />);
      const row = toggleRow("memory.export");
      expect(row).toHaveAttribute("aria-expanded", "true");

      fireEvent.click(screen.getByRole("button", { name: "Copy id" }));

      expect(writeText).toHaveBeenCalledWith(alpha);
      expect(await screen.findByRole("button", { name: "Copied" })).toBeInTheDocument();
      expect(row).toHaveAttribute("aria-expanded", "true");
    } finally {
      if (original) Object.defineProperty(navigator, "clipboard", original);
      else delete (navigator as { clipboard?: unknown }).clipboard;
    }
  });

  it("filters by device, opens a line, and clears a filter that matches nothing", () => {
    render(<AuditTable lines={lines} devices={devices} tokens={[]} />);
    expect(auditRows()).toHaveLength(3);

    fireEvent.change(screen.getByLabelText("Device"), { target: { value: alpha } });
    expect(auditRows()).toHaveLength(2);

    const row = toggleRow("memory.export");
    expect(row).toHaveAttribute("aria-expanded", "true");

    const record = document.querySelector(".audit-record") as HTMLElement;
    expect(record).not.toBeNull();
    expect(within(record).getByText("memory.export")).toBeInTheDocument();
    expect(within(record).getByTitle(new Date(lines[0].ts_ms).toISOString())).toBeInTheDocument();
    expect(within(record).getByText("200")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Tool or detail"), { target: { value: "zzz-no-such-tool" } });
    expect(
      screen.getByText("No calls match the machine Alpha and the search “zzz-no-such-tool”.")
    ).toBeInTheDocument();
    expect(auditRows()).toHaveLength(0);

    fireEvent.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(auditRows()).toHaveLength(3);
    expect(screen.queryByText(/No calls match/)).not.toBeInTheDocument();
  });

  it("names the machine scope when it hides the fleet's calls, and clears back to the fleet", () => {
    render(
      <AuditTable lines={[lines[0]]} devices={devices} tokens={[]} selectedDeviceId={beta} />
    );
    expect(screen.getByText("No calls match the machine Beta.")).toBeInTheDocument();
    expect(auditRows()).toHaveLength(0);

    fireEvent.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(auditRows()).toHaveLength(1);
  });

  it("tells an empty audit apart from a filter with no matches", () => {
    render(<AuditTable lines={[]} devices={devices} tokens={[]} />);
    expect(screen.getByText(/No calls yet\./)).toBeInTheDocument();
    expect(
      screen.getByText(/straight to a machine's own endpoint is not/)
    ).toBeInTheDocument();
    expect(screen.getByText(/Connect a client/)).toBeInTheDocument();
    expect(screen.queryByText(/No calls match/)).not.toBeInTheDocument();
  });


  it("names the caller's log when a filter matches nothing", () => {
    render(
      <AuditTable
        lines={[line({ detail: "bad_operator_token" })]}
        devices={[]}
        tokens={[]}
        empty="No failed authentications yet."
        emptyFiltered="No failed authentication matches this filter."
      />
    );
    fireEvent.change(screen.getByLabelText("Tool or detail"), {
      target: { value: "no-such-detail" }
    });
    expect(
      screen.getByText("No failed authentication matches this filter.")
    ).toBeInTheDocument();
  });

  it("hides the token filter when no line in the window carries a token", () => {
    const { rerender } = render(
      <AuditTable lines={[line({ detail: "bad_operator_token" })]} devices={[]} tokens={[]} />
    );
    expect(screen.queryByLabelText("Token")).not.toBeInTheDocument();

    rerender(
      <AuditTable
        lines={[line({ detail: "tool/call", token_id: "token-1" })]}
        devices={[]}
        tokens={[]}
      />
    );
    expect(screen.getByLabelText("Token")).toBeInTheDocument();
  });

  it("uses the caller's empty sentence instead of the audit's", () => {
    render(
      <AuditTable
        lines={[]}
        devices={devices}
        tokens={[]}
        empty="No failed authentications yet."
      />
    );
    expect(screen.getByText("No failed authentications yet.")).toBeInTheDocument();
    expect(screen.queryByText("No calls yet.")).not.toBeInTheDocument();
  });

  it("renders the bounded window without a nested scroller and without a caption", () => {
    const many: AuditLine[] = Array.from({ length: 200 }, (_, index) => ({
      ts_ms: index,
      kind: "tool",
      device_id: alpha,
      tool: "terminal.list",
      detail: null,
      status: 200
    }));
    render(<AuditTable lines={many} devices={devices} tokens={[]} />);

    expect(auditRows()).toHaveLength(200);
    expect(document.querySelector(".audit-table")?.closest(".scroll")).toBeNull();
    expect(screen.queryByText(/loaded the newest/)).not.toBeInTheDocument();
  });

  it("keeps an open row open when a filter removes the rows above it", () => {
    render(<AuditTable lines={lines} devices={devices} tokens={[]} />);
    const row = toggleRow("terminal.list");
    expect(row).toHaveAttribute("aria-expanded", "true");

    fireEvent.change(screen.getByLabelText("Tool or detail"), {
      target: { value: "terminal.list" }
    });
    expect(auditRows()).toHaveLength(1);
    expect(disclosureOf(auditRows()[0])).toHaveAttribute("aria-expanded", "true");
  });

  it("marks a device error that arrived as HTTP 200", () => {
    const refused: AuditLine = {
      ts_ms: 5,
      kind: "mcp",
      device_id: alpha,
      tool: "terminal.create",
      detail: "jsonrpc error -32002",
      status: 200
    };
    render(<AuditTable lines={[refused]} devices={devices} tokens={[]} />);

    const status = screen.getByText("200");
    expect(status).toHaveClass("chip-warn");
    expect(status.getAttribute("title")).toMatch(/JSON-RPC error/);
    expect(screen.getByText("error")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Status"), { target: { value: "failed" } });
    expect(auditRows()).toHaveLength(1);
  });

  it("offers a retired device by the prefix of its id", () => {
    const retired = "retired-device-0123456789";
    render(
      <AuditTable
        lines={[
          { ts_ms: 1, kind: "tool", device_id: retired, tool: null, detail: "did a thing", status: 200 }
        ]}
        devices={devices}
        tokens={[]}
      />
    );
    expect(screen.getByRole("option", { name: "retired-devi…" })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Device"), { target: { value: retired } });
    expect(auditRows()).toHaveLength(1);
  });

  it("names the token that made a call, and shows an em dash when no token did", () => {
    render(
      <AuditTable
        lines={[
          {
            ts_ms: 1,
            kind: "mcp",
            device_id: alpha,
            tool: "tools/list",
            detail: null,
            status: 200,
            token_id: tokenA
          },
          { ts_ms: 2, kind: "mcp", device_id: alpha, tool: "tools/list", detail: null, status: 200 }
        ]}
        devices={devices}
        tokens={[]}
      />
    );

    expect(within(screen.getByRole("table")).getByText(shortA)).toBeInTheDocument();
    expect(screen.getByText("—")).toBeInTheDocument();
    expect(screen.queryByText(/srm_/)).not.toBeInTheDocument();
  });

  it("names the token in the record, by the operator's name or its short id", () => {
    const unknown = "c".repeat(64);
    render(
      <AuditTable
        lines={[
          { ts_ms: 1, kind: "mcp", device_id: alpha, tool: "tools/list", detail: null, status: 200, token_id: tokenA, caller: "client" },
          { ts_ms: 2, kind: "mcp", device_id: alpha, tool: "files.read", detail: null, status: 200, token_id: unknown, caller: "client" }
        ]}
        devices={devices}
        tokens={tokens}
      />
    );

    toggleRow("tools/list");
    const named = recordOf("tools/list");
    expect(within(named).getByText("Token").nextElementSibling).toHaveTextContent("laptop-dsh");

    toggleRow("files.read");
    const unknownRecord = recordOf("files.read");
    expect(within(unknownRecord).getByText("Token").nextElementSibling).toHaveTextContent(
      "c".repeat(12) + "…"
    );
    expect(within(named).getByText("Token").nextElementSibling).not.toHaveTextContent(/srm_/);
  });

  it("says signed grant for a client with no token, and operator session only for the console", () => {
    render(
      <AuditTable
        lines={[
          { ts_ms: 1, kind: "mcp", device_id: alpha, tool: "terminal.list", detail: null, status: 200, caller: "console" },
          { ts_ms: 2, kind: "mcp", device_id: alpha, tool: "files.read", detail: null, status: 200, caller: "client" },
          { ts_ms: 3, kind: "token", device_id: alpha, tool: null, detail: "minted", status: 201 }
        ]}
        devices={devices}
        tokens={[]}
      />
    );

    toggleRow("terminal.list");
    toggleRow("files.read");
    const consoleRecord = within(recordOf("terminal.list")).getByText("Token").nextElementSibling;
    const clientRecord = within(recordOf("files.read")).getByText("Token").nextElementSibling;
    expect(consoleRecord).toHaveTextContent("operator session");
    expect(clientRecord).toHaveTextContent("signed grant");
    expect(clientRecord).not.toHaveTextContent("operator session");
  });

  it("offers Manage where a token is named, and only there", () => {
    const onManageToken = vi.fn();
    render(
      <AuditTable
        lines={[
          { ts_ms: 1, kind: "mcp", device_id: alpha, tool: "tools/list", detail: null, status: 200, token_id: tokenA, caller: "client" },
          { ts_ms: 2, kind: "mcp", device_id: alpha, tool: "files.read", detail: null, status: 200, caller: "client" }
        ]}
        devices={devices}
        tokens={tokens}
        onManageToken={onManageToken}
      />
    );

    toggleRow("tools/list");
    fireEvent.click(within(recordOf("tools/list")).getByRole("button", { name: "Manage" }));
    expect(onManageToken).toHaveBeenCalledWith(tokenA);

    toggleRow("files.read");
    expect(within(recordOf("files.read")).queryByRole("button", { name: "Manage" })).toBeNull();
  });

  it("keeps two otherwise identical lines apart when only their token differs", () => {
    const base: AuditLine = {
      ts_ms: 3,
      kind: "mcp",
      device_id: alpha,
      tool: "tools/call",
      detail: "ok",
      status: 200
    };
    const { rerender } = render(
      <AuditTable
        lines={[{ ...base, token_id: tokenA }, { ...base, token_id: tokenB }]}
        devices={devices}
        tokens={[]}
      />
    );

    fireEvent.click(disclosureOf(rowOf(shortA)));
    expect(disclosureOf(rowOf(shortA))).toHaveAttribute("aria-expanded", "true");

    rerender(
      <AuditTable
        lines={[{ ...base, token_id: tokenB }, { ...base, token_id: tokenA }]}
        devices={devices}
        tokens={[]}
      />
    );

    expect(disclosureOf(rowOf(shortA))).toHaveAttribute("aria-expanded", "true");
    expect(disclosureOf(rowOf(shortB))).toHaveAttribute("aria-expanded", "false");
  });

  it("names the token that made a call, and keeps the short id when the list does not know it", () => {
    const unknown = "c".repeat(64);
    render(
      <AuditTable
        lines={[
          { ts_ms: 1, kind: "mcp", device_id: alpha, tool: "tools/list", detail: null, status: 200, token_id: tokenA },
          { ts_ms: 2, kind: "mcp", device_id: alpha, tool: "tools/list", detail: null, status: 200, token_id: unknown },
          { ts_ms: 3, kind: "mcp", device_id: alpha, tool: "tools/list", detail: null, status: 200 }
        ]}
        devices={devices}
        tokens={tokens}
      />
    );

    const table = within(screen.getByRole("table"));
    expect(table.getByText("laptop-dsh")).toBeInTheDocument();
    expect(table.getByText("c".repeat(12) + "…")).toBeInTheDocument();
    expect(table.queryByText(shortA)).not.toBeInTheDocument();
    expect(table.getByText("—")).toBeInTheDocument();
    expect(screen.queryByText(/srm_/)).not.toBeInTheDocument();
  });

  it("filters by a token, naming the options the list knows and shortening the ones it does not", () => {
    const unknown = "c".repeat(64);
    const attributed: AuditLine[] = [
      { ts_ms: 1, kind: "mcp", device_id: alpha, tool: "tools/call", detail: null, status: 200, token_id: tokenB },
      { ts_ms: 2, kind: "mcp", device_id: beta, tool: "tools/call", detail: null, status: 200, token_id: tokenA },
      { ts_ms: 3, kind: "mcp", device_id: alpha, tool: "tools/call", detail: null, status: 200, token_id: unknown },
      { ts_ms: 4, kind: "mcp", device_id: alpha, tool: "tools/call", detail: null, status: 200 }
    ];
    render(<AuditTable lines={attributed} devices={devices} tokens={tokens} />);

    const select = screen.getByLabelText("Token");
    expect(within(select).getAllByRole("option").map((option) => option.textContent)).toEqual([
      "All tokens",
      "cccccccccccc…",
      "laptop-dsh",
      "spare-dsh"
    ]);
    expect(within(select).queryByRole("option", { name: /grant/i })).not.toBeInTheDocument();

    fireEvent.change(select, { target: { value: tokenA } });
    expect(auditRows()).toHaveLength(1);

    fireEvent.change(select, { target: { value: tokenB } });
    expect(auditRows()).toHaveLength(1);

    fireEvent.change(select, { target: { value: unknown } });
    expect(within(screen.getByRole("table")).getByText("cccccccccccc…")).toBeInTheDocument();
    expect(auditRows()).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(auditRows()).toHaveLength(4);
    expect((select as HTMLSelectElement).value).toBe("");
  });

  it("says which rows the console read for itself", () => {
    // Both rows are in the record; only one of them is the AI's work. The muted word beside the
    // machine is the whole difference, so a reader never has to guess who made a call.
    const consoleRead = line({
      ts_ms: Date.UTC(2026, 9, 26, 8, 0, 30),
      kind: "mcp",
      device_id: alpha,
      tool: "terminal.list",
      detail: null,
      status: 200,
      caller: "console"
    });
    const clientCall = line({
      ts_ms: Date.UTC(2026, 9, 26, 8, 0, 20),
      kind: "mcp",
      device_id: alpha,
      tool: "files.read",
      detail: null,
      status: 200,
      caller: "client"
    });
    render(<AuditTable lines={[consoleRead, clientCall]} devices={devices} tokens={[]} />);

    const consoleRow = auditRows().find((row) => row.textContent?.includes("terminal.list"));
    const clientRow = auditRows().find((row) => row.textContent?.includes("files.read"));
    expect(consoleRow?.querySelector(".audit-caller")).toHaveTextContent("console");
    expect(clientRow?.querySelector(".audit-caller")).toBeNull();
  });
});
