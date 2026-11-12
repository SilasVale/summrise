import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ServicesWatched } from "./ServicesWatched";
import { callTool, type Grant } from "./terminal";

vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);
const GRANT: Grant = { deviceId: "device-1", envelope: { token: "envelope" } };

/// The device's answer (`monitor_list` in `agent/src/plugins/monitor/tools.rs`): a target and `monitor::summary_of`'s payload,
/// where every fact is `null` until a probe exists.
const UP = {
  id: "9vh-console",
  host: "127.0.0.1",
  port: 19000,
  path: "/health",
  summary: {
    probes: 40,
    up: 39,
    down: 1,
    up_pct: 97.5,
    up_now: true,
    since_ms: 1_762_000_000_000,
    latency: { min_ms: 1, max_ms: 9, last_ms: 2 },
    drops: 1,
    last_status: 200,
    last_expect_ok: true
  }
};

const answer = (targets: unknown[]) => JSON.stringify({ ok: true, interval_secs: 15, targets });

beforeEach(() => mocked.mockReset());

describe("ServicesWatched — what the device probes", () => {
  it("draws a target with its address, its state word, and the summary's numbers", async () => {
    mocked.mockResolvedValue(answer([UP]));
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() => expect(screen.getByText("Up")).toBeInTheDocument());
    expect(mocked).toHaveBeenCalledWith(GRANT, "monitor.list", {});
    expect(screen.getByText("127.0.0.1:19000/health")).toBeInTheDocument();
    expect(screen.getByText("98% of 40 probes")).toBeInTheDocument();
    expect(screen.getByText("1 drop")).toBeInTheDocument();
    expect(screen.getByText("HTTP 200")).toBeInTheDocument();
    expect(screen.getByText(/probed every 15 s/)).toBeInTheDocument();
  });

  it("reads the BARE ARRAY the shipped device answers with, and draws it as empty", async () => {
    // Measured on the live fleet 2026-11-12: `monitor.list` on 0.3.292 answers `[]`, not `{ok, targets}`, and the page said
    // "the machine answered monitor.list without a list" for a machine with nothing to watch.
    mocked.mockResolvedValue("[]");
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() =>
      expect(screen.getByText("This machine watches no services.")).toBeInTheDocument()
    );
  });

  it("draws an empty watch list as a sentence, not a blank section", async () => {
    mocked.mockResolvedValue(answer([]));
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() =>
      expect(screen.getByText("This machine watches no services.")).toBeInTheDocument()
    );
  });

  it("never calls an unprobed target DOWN — a null is its own word", async () => {
    mocked.mockResolvedValue(
      answer([
        {
          id: "new",
          host: "example.test",
          port: 443,
          path: null,
          summary: { probes: 0, up: 0, down: 0, up_pct: null, up_now: null, since_ms: null, drops: null, last_status: null }
        }
      ])
    );
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() => expect(screen.getByText("Not probed")).toBeInTheDocument());
    expect(screen.queryByText("Down")).toBeNull();
    expect(screen.queryByText("0% of 0 probes")).toBeNull();
  });

  it("says a refused read in the device's words and offers the retry", async () => {
    mocked.mockResolvedValueOnce(JSON.stringify({ ok: false, error: "the watch list is unreadable" }));
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() =>
      expect(screen.getByText(/the watch list is unreadable/)).toBeInTheDocument()
    );
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("drops a target it cannot address, rather than drawing a blank row", async () => {
    mocked.mockResolvedValue(answer([UP, { id: "x", host: "", port: 80, summary: {} }]));
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() => expect(screen.getByText("Up")).toBeInTheDocument());
    expect(screen.getByText("One target · probed every 15 s on the machine")).toBeInTheDocument();
  });
});
