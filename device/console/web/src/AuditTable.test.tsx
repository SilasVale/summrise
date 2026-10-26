import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { AuditLine, Device } from "./api";
import { AuditTable } from "./AuditTable";

const alpha = "device-a-0123456789ab";
const beta = "device-b-0123456789ab";

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

const detailButtons = () => screen.getAllByRole("button", { name: "Details" });

// Two record ids as the console writes them: 64 lowercase hex, never the `srm_` secret.
const tokenA = "a".repeat(64);
const tokenB = "b".repeat(64);
const shortA = "a".repeat(12) + "…";
const shortB = "b".repeat(12) + "…";

/** The table row that carries a token cell with this text. */
function rowOf(text: string): HTMLElement {
  const row = screen.getByText(text).closest("tr");
  expect(row).not.toBeNull();
  return row as HTMLElement;
}


describe("AuditTable", () => {
  it("filters by device, opens a line, and clears a filter that matches nothing", () => {
    render(<AuditTable lines={lines} devices={devices} />);
    expect(detailButtons()).toHaveLength(3);

    fireEvent.change(screen.getByLabelText("Device"), { target: { value: alpha } });
    expect(screen.getByText(/2 of 3 lines/)).toBeInTheDocument();
    expect(detailButtons()).toHaveLength(2);

    fireEvent.click(detailButtons()[0]);
    expect(screen.getByRole("button", { name: "Hide" })).toHaveAttribute("aria-expanded", "true");

    const details = screen.getByText(alpha).closest("dl");
    expect(details).not.toBeNull();
    const expanded = within(details as HTMLElement);
    expect(expanded.getByText("memory.export")).toBeInTheDocument();
    expect(expanded.getByText(new Date(lines[0].ts_ms).toISOString())).toBeInTheDocument();
    expect(expanded.getByText("200")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Tool or detail"), { target: { value: "zzz-no-such-tool" } });
    expect(screen.getByText("No audit lines match.")).toBeInTheDocument();
    expect(screen.getByText(/0 of 3 lines/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(detailButtons()).toHaveLength(3);
    expect(screen.getByText(/3 of 3 lines/)).toBeInTheDocument();
    expect(screen.queryByText("No audit lines match.")).not.toBeInTheDocument();
  });

  it("tells an empty audit apart from a filter with no matches", () => {
    render(<AuditTable lines={[]} devices={devices} />);
    expect(screen.getByText("No audit lines yet.")).toBeInTheDocument();
    expect(screen.queryByText("No audit lines match.")).not.toBeInTheDocument();
  });

  it("says the page shows a bounded window when the window is full", () => {
    const many: AuditLine[] = Array.from({ length: 200 }, (_, index) => ({
      ts_ms: index,
      kind: "tool",
      device_id: alpha,
      tool: "terminal.list",
      detail: null,
      status: 200
    }));
    render(<AuditTable lines={many} devices={devices} />);
    const caption = screen.getByText(/loaded the newest 200 lines/);
    expect(caption).toHaveTextContent(/older lines are not loaded/);
    // The page knows how many it asked for, not what the console holds: it must not claim there are more.
    expect(caption).not.toHaveTextContent(/keeps more/);
  });

  it("keeps an open row open when a filter removes the rows above it", () => {
    render(<AuditTable lines={lines} devices={devices} />);
    fireEvent.change(screen.getByLabelText("Device"), { target: { value: alpha } });
    fireEvent.click(detailButtons()[1]);
    expect(screen.getByRole("button", { name: "Hide" })).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Tool or detail"), {
      target: { value: "terminal.list" }
    });
    expect(screen.getByRole("button", { name: "Hide" })).toBeInTheDocument();
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
    render(<AuditTable lines={[refused]} devices={devices} />);

    const status = screen.getByText("200");
    expect(status).toHaveClass("chip-warn");
    expect(status.getAttribute("title")).toMatch(/JSON-RPC error/);
    expect(screen.getByText("error")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Status"), { target: { value: "failed" } });
    expect(screen.getByText(/1 of 1 lines/)).toBeInTheDocument();
  });

  it("offers a retired device by the prefix of its id", () => {
    const retired = "retired-device-0123456789";
    render(
      <AuditTable
        lines={[
          { ts_ms: 1, kind: "tool", device_id: retired, tool: null, detail: "did a thing", status: 200 }
        ]}
        devices={devices}
      />
    );
    expect(screen.getByRole("option", { name: "retired-devi…" })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Device"), { target: { value: retired } });
    expect(screen.getByText(/1 of 1 lines/)).toBeInTheDocument();
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
      />
    );

    expect(screen.getByText(shortA)).toBeInTheDocument();
    expect(screen.getByText("—")).toBeInTheDocument();
    // The id is the record, not the credential: the table never renders an `srm_` value.
    expect(screen.queryByText(/srm_/)).not.toBeInTheDocument();
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
      <AuditTable lines={[{ ...base, token_id: tokenA }, { ...base, token_id: tokenB }]} devices={devices} />
    );

    fireEvent.click(within(rowOf(shortA)).getByRole("button", { name: "Details" }));
    expect(within(rowOf(shortA)).getByRole("button", { name: "Hide" })).toBeInTheDocument();

    // The poll returns new objects, and a line's order can change: the open row is the token's row,
    // not whichever line now happens to hold the same ordinal.
    rerender(
      <AuditTable lines={[{ ...base, token_id: tokenB }, { ...base, token_id: tokenA }]} devices={devices} />
    );

    expect(within(rowOf(shortA)).getByRole("button", { name: "Hide" })).toBeInTheDocument();
    expect(within(rowOf(shortB)).getByRole("button", { name: "Details" })).toBeInTheDocument();
  });
});
