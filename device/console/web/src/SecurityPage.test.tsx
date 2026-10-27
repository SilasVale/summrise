import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AuditLine, Device } from "./api";
import { SecurityPage } from "./SecurityPage";

// The fleet-token form asks for the registry; the denial tests do not need a network round trip.
vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    api: { ...actual.api, tools: vi.fn().mockResolvedValue([]) }
  };
});

const workshop: Device = {
  id: "device-workshop-0123456789",
  name: "workshop",
  online: true,
  version: "0.3.42",
  last_seen_ms: 1
};

// The shape the console writes to denials.jsonl: the audit Entry, with no token_id because a refused
// caller never authenticated, and the short code in `detail` rather than a credential.
const denied: AuditLine = {
  ts_ms: Date.UTC(2026, 9, 26, 8, 0, 0),
  kind: "denied",
  device_id: null,
  tool: null,
  detail: "bad_operator_token",
  status: 401
};

const suppressed: AuditLine = {
  ts_ms: Date.UTC(2026, 9, 26, 8, 1, 0),
  kind: "denied",
  device_id: null,
  tool: null,
  detail: "suppressed 12",
  status: 429
};

describe("SecurityPage", () => {
  it("says what the log is, including the rate limit and the suppression summary", () => {
    render(<SecurityPage lines={[]} devices={[]} tokens={[]} />);

    expect(screen.getByRole("heading", { name: "Security" })).toBeInTheDocument();
    expect(screen.getByText(/rate-limited to 100\/minute/)).toBeInTheDocument();
    expect(
      screen.getByText(/suppressed N line means N refusals were not written/)
    ).toBeInTheDocument();
    expect(screen.getByText("No failed authentications yet.")).toBeInTheDocument();
    expect(screen.queryByText("No audit lines yet.")).not.toBeInTheDocument();
  });

  it("names a filter that matches no failed authentication", () => {
    render(<SecurityPage lines={[denied]} devices={[]} tokens={[]} />);
    fireEvent.change(screen.getByLabelText("Status"), { target: { value: "ok" } });

    expect(
      screen.getByText("No failed authentication matches this filter.")
    ).toBeInTheDocument();
    expect(screen.queryByText("No audit lines match.")).not.toBeInTheDocument();
  });

  it("renders the refusal log in the audit table, with no token named", () => {
    render(<SecurityPage lines={[denied, suppressed]} devices={[workshop]} tokens={[]} />);

    expect(screen.queryByText("No failed authentications yet.")).not.toBeInTheDocument();
    const table = within(screen.getByRole("table"));
    expect(table.getAllByText("denied")).toHaveLength(2);
    expect(table.getByText("bad_operator_token")).toBeInTheDocument();
    expect(table.getByText("401")).toBeInTheDocument();
    expect(table.getByText("suppressed 12")).toBeInTheDocument();
    expect(table.getByText("429")).toBeInTheDocument();
    // Denials carry no token id: the token column is the table's em dash, not a blank cell.
    expect(table.getAllByText("—")).toHaveLength(2);
    expect(screen.queryByText(/srm_/)).not.toBeInTheDocument();
  });
});
