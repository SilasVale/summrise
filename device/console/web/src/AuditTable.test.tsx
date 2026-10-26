import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { AuditLine, Device, TokenMeta } from "./api";
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

/** The table row that carries a token cell with this text (the token filter may show it too). */
function rowOf(text: string): HTMLElement {
  const row = within(screen.getByRole("table")).getByText(text).closest("tr");
  expect(row).not.toBeNull();
  return row as HTMLElement;
}


describe("AuditTable", () => {
  it("filters by device, opens a line, and clears a filter that matches nothing", () => {
    render(<AuditTable lines={lines} devices={devices} tokens={[]} />);
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
    render(<AuditTable lines={[]} devices={devices} tokens={[]} />);
    expect(screen.getByText("No audit lines yet.")).toBeInTheDocument();
    expect(screen.queryByText("No audit lines match.")).not.toBeInTheDocument();
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
    expect(screen.queryByText("No audit lines yet.")).not.toBeInTheDocument();
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
    render(<AuditTable lines={many} devices={devices} tokens={[]} />);
    const caption = screen.getByText(/loaded the newest 200 lines/);
    expect(caption).toHaveTextContent(/older lines are not loaded/);
    // The page knows how many it asked for, not what the console holds: it must not claim there are more.
    expect(caption).not.toHaveTextContent(/keeps more/);
  });

  it("keeps an open row open when a filter removes the rows above it", () => {
    render(<AuditTable lines={lines} devices={devices} tokens={[]} />);
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
    render(<AuditTable lines={[refused]} devices={devices} tokens={[]} />);

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
        tokens={[]}
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
        tokens={[]}
      />
    );

    expect(within(screen.getByRole("table")).getByText(shortA)).toBeInTheDocument();
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
      <AuditTable
        lines={[{ ...base, token_id: tokenA }, { ...base, token_id: tokenB }]}
        devices={devices}
        tokens={[]}
      />
    );

    fireEvent.click(within(rowOf(shortA)).getByRole("button", { name: "Details" }));
    expect(within(rowOf(shortA)).getByRole("button", { name: "Hide" })).toBeInTheDocument();

    // The poll returns new objects, and a line's order can change: the open row is the token's row,
    // not whichever line now happens to hold the same ordinal.
    rerender(
      <AuditTable
        lines={[{ ...base, token_id: tokenB }, { ...base, token_id: tokenA }]}
        devices={devices}
        tokens={[]}
      />
    );

    expect(within(rowOf(shortA)).getByRole("button", { name: "Hide" })).toBeInTheDocument();
    expect(within(rowOf(shortB)).getByRole("button", { name: "Details" })).toBeInTheDocument();
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
    // The name is the convenience; the id is the fact, and an id the list cannot resolve is not blank.
    expect(table.getByText("c".repeat(12) + "…")).toBeInTheDocument();
    expect(table.queryByText(shortA)).not.toBeInTheDocument();
    expect(table.getByText("—")).toBeInTheDocument();
    // Nothing on this table is the credential.
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
    // Sorted by the label the operator reads: a name where the list has one, the short id where it
    // does not, and All tokens first.
    expect(within(select).getAllByRole("option").map((option) => option.textContent)).toEqual([
      "All tokens",
      "cccccccccccc…",
      "laptop-dsh",
      "spare-dsh"
    ]);
    // The grant-authenticated line has no token and is not in any token's option list.
    expect(within(select).queryByRole("option", { name: /grant/i })).not.toBeInTheDocument();

    fireEvent.change(select, { target: { value: tokenA } });
    expect(screen.getByText(/1 of 4 lines/)).toBeInTheDocument();
    expect(detailButtons()).toHaveLength(1);

    fireEvent.change(select, { target: { value: tokenB } });
    expect(screen.getByText(/1 of 4 lines/)).toBeInTheDocument();

    fireEvent.change(select, { target: { value: unknown } });
    expect(within(screen.getByRole("table")).getByText("cccccccccccc…")).toBeInTheDocument();
    expect(screen.getByText(/1 of 4 lines/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(screen.getByText(/4 of 4 lines/)).toBeInTheDocument();
    expect((select as HTMLSelectElement).value).toBe("");
  });
});
