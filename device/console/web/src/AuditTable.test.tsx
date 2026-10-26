import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
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

afterEach(cleanup);

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
    expect(screen.getByText(/shows the newest 200 lines/)).toHaveTextContent(/console keeps more/);
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
});
