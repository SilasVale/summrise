import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DeviceSelfView } from "./DeviceSelfView";
import { callTool, type Grant } from "./terminal";

vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);
const GRANT: Grant = { deviceId: "device-1", envelope: { token: "envelope" } };

/// `state.get`'s answer, read from the LIVE device 2026-11-12: health and release beside the last events it recorded.
const LIVE = {
  status: "ok",
  release: "0.3.292",
  pid: 4242,
  state: { anything: true },
  events: [
    { event: "begin", id: 987, tool: "plugins.list", decision: "allow", transport: "console", ts_ms: 1_794_477_517_671 },
    { event: "end", id: 987, ok: true, transport: "console", ts_ms: 1_794_477_517_672 }
  ]
};

beforeEach(() => mocked.mockReset());

describe("DeviceSelfView — what the machine says about itself", () => {
  it("reads once when shown, then draws the health line and its events", async () => {
    mocked.mockResolvedValue(JSON.stringify(LIVE));
    render(<DeviceSelfView grant={GRANT} />);
    await waitFor(() => expect(screen.getByText(/release 0\.3\.292/)).toBeInTheDocument());
    expect(mocked).toHaveBeenCalledWith(GRANT, "state.get", {});
    expect(screen.getByText("begin plugins.list")).toBeInTheDocument();
    expect(screen.getByText(/console · ok/)).toBeInTheDocument();
    expect(screen.getByText(/allow · console/)).toBeInTheDocument();
  });

  it("draws a poisoned document as a degradation, not as an error", async () => {
    mocked.mockResolvedValue(
      JSON.stringify({ status: "degraded", release: "0.3.292", pid: 7, state: null, events: [] })
    );
    render(<DeviceSelfView grant={GRANT} />);
    await waitFor(() =>
      expect(screen.getByText(/its state document could not be read/)).toBeInTheDocument()
    );
    expect(screen.getByText("The machine has recorded no events yet.")).toBeInTheDocument();
  });

  it("says a refused read in the device's words and offers the retry", async () => {
    mocked.mockResolvedValueOnce(JSON.stringify({ ok: false, error: "state is unreadable" }));
    render(<DeviceSelfView grant={GRANT} />);
    await waitFor(() => expect(screen.getByText(/state is unreadable/)).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });
});
